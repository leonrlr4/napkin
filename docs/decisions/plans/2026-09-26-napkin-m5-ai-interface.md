# napkin M5：AI 介面 Implementation Plan

> Historical record, frozen 2026-09-26. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude Code 透過 `napkin` 子指令讀取、修改、看到正在開的畫布：一批修改是一步 undo、立刻出現在畫面上；napkin 裡按 `Ctrl+K` 開出跑 Claude Code 的浮動終端機。

**Architecture:** `scene` 新增純資料的三層：`scene::text`（Excalidraw 的文字量測規則，字寬由呼叫端注入的 `TextMeasure` 提供）、`scene::binding`（箭頭綁定的 `fixedPoint`）、`scene::batch`（`apply` 的一批操作：skeleton 轉換、update、delete，整批在複本上執行，全部成功才交回），`Editor::apply_batch` 把結果記成一步 undo。`app` 新增 `app::control`：請求與回應的格式、不依賴 GUI 的請求處理函數、摘要格式、PNG 輸出規劃、Unix socket 伺服器與子指令用戶端。`NapkinApp` 在 UI 執行緒逐幀處理佇列裡的請求，用自己的離螢幕 `CanvasRenderer` 畫 PNG。

**Tech Stack:** 沿用 M4a：Rust 1.98.1（edition 2024）、eframe／egui／egui-wgpu 0.36.2、wgpu 30、glyphon 0.12.0、serde_json 1。新增：`app` 依賴 workspace 已有的 `serde`，以及 `png = "0.18"`（Cargo.lock 已經有 0.18.1，是 eframe 的間接依賴）。

**Spec:** `docs/decisions/specs/2026-09-26-napkin-ai-design.md`（以下稱 AI spec）；AI spec 引用的主 spec 是 `docs/decisions/specs/2026-09-13-napkin-design.md`。

**前置：** M4a 已合併進 `master`（PR #3）。分支 `m5-ai-interface` 已從 `master` 開好，在原本的 checkout 上工作，不開 worktree。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號或計畫。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 移植的行為以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`。port 時看 JS 原始碼，不看這份計畫的摘要；計畫裡的程式碼與 JS 不一致時，照 JS 改，並在回報裡寫出差異。
- `scene` 與 `rough` 不能依賴 egui、wgpu、glyphon 或任何繪圖、視窗 crate。字寬一律透過 `scene::text::TextMeasure` 從外面注入。
- JS 數值語意走 `rough::js`；亂數與時間走 `scene::env::Env`；`scene` 裡不直接呼叫 `getrandom` 或 `SystemTime`。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫（`mutateElement` 的 `didChange`）。新建立的元件照 JS 的 `Object.assign`（不 bump）或 `mutateElement`（bump）區分，JS 基準會檢查 `version`。
- `Element::Raw` 只能改 `x`、`y`，以及刪除（主 spec §5.2），而且只透過 `Element` 既有的方法。
- 無法解析的檔案絕不寫入（主 spec §8）；畫布唯讀時，修改類請求回錯誤。`--bench` 不開 socket。
- socket 路徑 `$XDG_RUNTIME_DIR/napkin.sock`，權限 0600。環境變數 `NAPKIN_SOCKET` 設定時改用它（整合測試用）。
- napkin 沒在執行時，子指令印 `napkin is not running; open it with SUPER+N` 到 stderr，結束碼 1，不碰任何檔案。
- `Ctrl+K`：`xdg-terminal-exec --app-id=org.napkin.agent --dir=$HOME/Documents/napkin -- claude --continue --dangerously-skip-permissions --model sonnet`；已有 class 為 `org.napkin.agent` 的視窗時改成聚焦它。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試需要 wgpu adapter，沒有 adapter 時直接失敗，不跳過。改了 `tools/baseline/scene/cases.mjs` 或 generator 的任務，要執行 `cd tools/baseline && npm run scene` 並 commit 產出的 JSON；沒改 case 的群組，重新產生後 `git diff` 必須是空的。
- GUI 煙霧測試只做開視窗、截圖、關閉（`hyprctl dispatch 'hl.dsp.focus({ window = "class:napkin" })'`、`grim`、`hyprctl dispatch 'hl.dsp.window.close()'`）。不要用 `wtype` 或任何方式送按鍵或滑鼠事件，互動驗收交給使用者。

## 與 spec 不同的地方

1. **一批裡的 `add` 先全部執行，再照順序執行 `update` 與 `delete`。** Excalidraw 的 `convertToExcalidrawElements` 一次處理整組 skeleton，箭頭可以指向排在它後面的形狀；napkin 保留這個語意，所以 add 不能跟 update／delete 交錯。`update`／`delete` 可以用同一批 add 定義的代號。
2. **形狀一律要給 `width`、`height`**（大於 0）。Excalidraw 在有標籤而沒給寬高時用 0 再由 `redrawTextBoundingBox` 撐大，napkin 不撐大容器（AI spec §4.5），所以直接要求寬高。
3. **線與箭頭的 `points` 先正規化成第一點在 `[0, 0]`**，偏移加到 `x`、`y`。Excalidraw 的 skeleton 轉換不正規化，而 `bindLinearElementToElement` 的 0.5 位移會直接覆寫第一點的座標；napkin 的編輯程式假設第一點是 `[0, 0]`。
4. **線與手繪筆畫的 `width`／`height` 由 `points` 算。** Excalidraw 對 `line` 保留傳進來的寬高（沒給就是 100），對 `freedraw` 是 0；napkin 的外框與選取靠這兩個值。箭頭照 Excalidraw，由 `getSizeFromPoints` 算，0.5 位移之後不再更新。
5. **沒給 `points` 的線或箭頭，`width`／`height` 有給就照用，包括 0**；兩個都沒給才是 100 與 0。Excalidraw 用 `element.width || 100`，給 `width: 0, height: 80` 會變成 100×80 的斜線。
6. **箭頭可以指定 `startArrowhead`／`endArrowhead`。** AI spec §4.2 的共用欄位沒有列，但它是 `ExcalidrawElementSkeleton` 的一部分，雙向箭頭與無箭頭的連線需要它。
7. **標籤放不下時回傳警告。** AI spec 不自動換行、不撐大容器；`apply` 的回應多一個 `warnings`，列出哪個標籤需要多大、容器目前放得下多大，讓 Claude 自己調整。
8. **`update` 改了箭頭的位置或點、或改了被箭頭綁定的形狀的位置或大小，重新計算該端的 `fixedPoint`。** 箭頭仍然不跟著形狀走（M6）；重新計算只是讓 `fixedPoint` 對應畫面上的端點，之後在 excalidraw.com 拖動形狀時箭頭不會跳開。
9. **修改類請求在使用者手勢進行中時排隊**，等 `Editor::is_idle()` 再執行，不中斷使用者正在拖的東西。用戶端最多等 120 秒。
10. **視窗標題與右上角都用家目錄縮寫成 `~` 的路徑。** AI spec §3.5 只對右上角要求縮寫；兩處一致比較好認，複製到剪貼簿的仍然是絕對路徑。

## 寫計畫時做的決定

1. **JS 基準涵蓋 `newTextElement` 與 skeleton 轉換。** Excalidraw 有 `setCustomTextMetricsProvider`，generator 安裝一個確定性的字寬（每個 UTF-16 code unit 寬 `0.6 × fontSize`），Rust 測試用同一個公式的 `scene::sample::CharWidthMeasure`。skeleton 轉換會呼叫 `Math.random`（`seed`、`versionNonce`），harness 的 `runCase` 加一個選項：兩次不同亂數流跑出完全相同的輸出時仍然逐數比較；generator 先刪掉 `seed`、`versionNonce`，id 依輸出順序改名成 `e0`、`e1`…（Rust 端做同樣的正規化）。
2. **基準只放標籤放得下的案例。** Excalidraw 在放不下時換行並撐大容器，napkin 刻意不做（AI spec §4.5），那部分由 Rust 單元測試鎖住。
3. **新元件的樣式用 `Editor::style()`（M4a 的 `ItemStyle`）**，所以圓角預設 `round`。基準測試用 `edges` 與 `arrow_type` 都是 `Sharp` 的 `ItemStyle`，對應 Excalidraw skeleton 的 `roundness: null`。標籤照 `bindTextToContainer`：樣式是 `DEFAULT_ELEMENT_PROPS`，只有 `strokeColor` 取容器的。
4. **請求處理函數 `control::handler::handle` 不碰 socket 與 GUI**：輸入一個 `Session`（`Editor`、存檔狀態、相機、字寬量測、點陣化介面）與 `Request`，輸出 `Response`。socket 伺服器只負責收發；`NapkinApp` 負責排隊、建 `Session`、提供 GPU 點陣化。
5. **協定**：每個連線一行 JSON 請求、一行 JSON 回應。請求 `{"command":"status"}`、`{"command":"scene","full":true}`、`{"command":"apply","batch":{...}}`、`{"command":"render","out":"/abs/x.png","target":"selection"}`；回應 `{"ok":true,"output":"..."}` 或 `{"ok":false,"output":"..."}`。`output` 是用戶端原樣印到 stdout（成功）或 stderr（失敗）的文字，格式全部由 napkin 端決定，用戶端很薄。
6. **摘要格式**（`napkin scene`、`napkin selection`）：第一行 `file <絕對路徑>`（沒有存檔路徑時 `file (none)`）；之後每個未刪除元件一行 `<id> <type> <x> <y> <w> <h>`，再接有值才出現的欄位：`label=<JSON 字串>`、`text=<JSON 字串>`、`stroke=<色>`（`#1e1e1e` 時省略）、`bg=<色>`（`transparent` 時省略）、`start=<id>`、`end=<id>`、`points=<JSON>`（只有線與箭頭）、`groups=<id,id>`、`angle=<弧度>`（0 時省略）、`locked`。數字四捨五入到小數兩位並去掉多餘的 0。綁定在活著的容器上的文字不單獨成行，以容器那行的 `label=` 出現。`--full` 改成第一行路徑、之後每個未刪除元件一行壓縮 JSON。
7. **`render` 的範圍**：`all` 是所有未刪除元件的外框，`selection` 只畫選取的元件與它們綁定的文字，`view` 是目前視窗。前兩者縮放 `min(2, 1536 / 長邊)`，四周留 16 px；`view` 用目前相機、視窗大小（邏輯像素）。長寬都不超過 4096 px。背景是 `viewBackgroundColor`，深色模式照目前主題。
8. **字寬量測在 `app::render::text::FontMeasure`**，用 cosmic-text 以 napkin 內建字型排版一行、取 `line_w`。它持有自己的 `FontSystem`，在第一次需要時建立，之後重用。
9. **PNG 用 napkin 自己的一個離螢幕 `CanvasRenderer`**（格式 `Rgba8Unorm`、4× MSAA），第一次 `render` 時建立。目前 `crates/app/tests/support` 裡的離螢幕渲染搬到 `app::render::offscreen`，測試與 app 共用。
10. **M4a 延後項目排進 M5 的三件**：Super 鍵（`wl_keyboard` 追蹤 Super 是否按著，按著時不把字母鍵交給編輯器）、重新載入失敗後檔案被刪就一直唯讀、`History::record` 每次深拷貝 2n 個元件（AI 每批都會記一筆）。其餘延後項目留在 memory `m4a-deferred`。
11. **skill 的 `apply` 例子由測試執行**：`crates/app/tests/skill_examples.rs` 抽出 `skills/napkin/SKILL.md` 裡每個 ```json 區塊，對空場景套用必須成功。

## 檔案結構

```
crates/scene/src/
  text.rs              TextMeasure、getLineHeight、normalizeText、measureText
  new_element.rs       + new_text_element
  binding.rs           fixedPoint 計算、bindBindingElement（非 elbow）
  batch/mod.rs         一批的解析、update、delete、OpError、BatchReport、apply_batch
  batch/add.rs         skeleton 轉換（convertToExcalidrawElements 的 napkin 子集）與標籤
  batch/validate.rs    欄位驗證（顏色、樣式列舉、數字、點）
  editor/mod.rs        + Editor::apply_batch
  editor/style.rs      + 從 create.rs 搬來的 roundness 與 ElementProps 輔助函數
  element.rs           + set_binding、add_bound_element
  transform.rs         + bound_text_max_size（從 bound_text_position 抽出）
  history.rs           diff 只複製有變的元件
  sample.rs            + CharWidthMeasure
crates/app/src/
  control/mod.rs       Request、Response、RenderTarget
  control/summary.rs   摘要格式
  control/handler.rs   Session、handle、is_mutating
  control/render.rs    Rasterize、render 範圍規劃、PNG 編碼
  control/server.rs    Unix socket 伺服器
  control/client.rs    子指令用戶端
  render/offscreen.rs  離螢幕渲染（從 tests/support 搬來）、GpuRasterizer
  render/text.rs       + FontMeasure
  agent.rs             Ctrl+K：聚焦或啟動終端機
  cli.rs               子指令解析
  main.rs              子指令走用戶端，不開視窗
  napkin_app.rs        控制 socket、請求佇列、路徑顯示、Ctrl+K、Super 鍵
  pinch.rs             + wl_keyboard 追蹤 Super
  edit_input.rs        Super 按著時忽略按鍵
  storage.rs           + display_path
crates/app/tests/
  control_socket.rs    無視窗 server + 真的執行 napkin 子指令
  gpu_render.rs        render 請求的 GPU 測試
  skill_examples.rs    SKILL.md 的 apply 例子
skills/napkin/SKILL.md
tools/baseline/        + newTextElement、skeleton 群組、runCase 選項、nanoid 計數器
```

---

### Task 1：文字量測與 `new_text_element`

**Files:**
- Create: `crates/scene/src/text.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/new_element.rs`、`crates/scene/src/sample.rs`、`crates/scene/tests/baseline.rs`、`crates/app/src/render/text.rs`
- Modify: `tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`；重新產生 `crates/scene/tests/baseline/new_element.json`

**Interfaces:**
- Consumes: `new_element::{ElementProps, new_base}`（`new_base` 是模組內的私有函數）、`element::TextElement`、`json::Slot`、`env::Env`。
- Produces:

```rust
// scene::text
pub const DEFAULT_FONT_FAMILY: f64 = 5.0; // FONT_FAMILY.Excalifont
pub const DEFAULT_FONT_SIZE: f64 = 20.0;
pub trait TextMeasure {
    /// `TextMetricsProvider.getLineWidth`: the width of one line (no `\n`) in scene units.
    fn line_width(&mut self, line: &str, font_family: f64, font_size: f64) -> f64;
}
pub fn line_height(font_family: f64) -> f64;           // getLineHeight
pub fn normalize_text(text: &str) -> String;           // normalizeText (normalizeEOL + tabs)
pub fn measure_text(text: &str, font_family: f64, font_size: f64, line_height: f64,
                    measure: &mut dyn TextMeasure) -> [f64; 2]; // measureText: [width, height]

// scene::new_element
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextProps {
    pub text: String,
    /// `None` (or a JS-falsy value) takes `newTextElement`'s default.
    pub font_size: Option<f64>,
    pub font_family: Option<f64>,
    pub text_align: Option<String>,
    pub vertical_align: Option<String>,
    pub container_id: Option<String>,
    pub line_height: Option<f64>,
}
pub fn new_text_element(props: ElementProps, text: TextProps,
                        measure: &mut dyn TextMeasure, env: &mut impl Env) -> Element;

// scene::sample
/// The baseline generator's text metrics: every UTF-16 code unit is `0.6 * fontSize` wide.
pub struct CharWidthMeasure;
impl TextMeasure for CharWidthMeasure { .. }

// app::render::text
pub struct FontMeasure { .. }
impl FontMeasure { pub fn new() -> FontMeasure; }
impl scene::text::TextMeasure for FontMeasure { .. }
```

要讀的 JS：`packages/element/src/textMeasurements.ts`（`measureText`、`getTextWidth`、`getTextHeight`、`normalizeText`、`setCustomTextMetricsProvider`）、`packages/common/src/utils.ts` 的 `normalizeEOL`、`packages/common/src/font-metadata.ts` 的 `FONT_METADATA` 與 `getLineHeight`、`packages/element/src/newElement.ts` 的 `newTextElement`、`getTextAnchorRatios`、`getTextElementPositionOffsets`。

- [ ] **Step 1：`text.rs` 的單元測試**

建立 `crates/scene/src/text.rs`，先只放測試與空函數（`todo!()`），`lib.rs` 加 `pub mod text;`：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::CharWidthMeasure;

    #[test]
    fn line_heights_follow_font_metadata() {
        assert_eq!(line_height(5.0), 1.25);
        assert_eq!(line_height(6.0), 1.25);
        assert_eq!(line_height(7.0), 1.15);
        assert_eq!(line_height(8.0), 1.25);
        assert_eq!(line_height(2.0), 1.15);
        assert_eq!(line_height(3.0), 1.2);
        assert_eq!(line_height(9.0), 1.15);
        assert_eq!(line_height(42.0), 1.25, "unknown ids fall back to Excalifont");
    }

    #[test]
    fn normalizes_line_endings_and_tabs() {
        assert_eq!(normalize_text("a\r\nb\rc\nd\te"), "a\nb\nc\nd        e");
    }

    #[test]
    fn measures_the_widest_line_and_counts_empty_lines() {
        let mut measure = CharWidthMeasure;
        // "abc" is 3 * 0.6 * 20 = 36 wide; the empty middle line counts as one line (" ").
        assert_eq!(measure_text("ab\n\nabc", 5.0, 20.0, 1.25, &mut measure), [36.0, 75.0]);
        assert_eq!(measure_text("", 5.0, 10.0, 1.2, &mut measure), [6.0, 12.0]);
    }
}
```

- [ ] **Step 2：確認測試失敗**

Run: `cargo test -p scene text::tests`
Expected: 編譯失敗（`CharWidthMeasure` 不存在）或 `todo!()` panic。

- [ ] **Step 3：實作 `text.rs` 與 `CharWidthMeasure`**

```rust
//! Text measurement as Excalidraw does it, ported from `packages/element/src/textMeasurements.ts`
//! (`measureText`, `getTextWidth`, `getTextHeight`, `normalizeText`),
//! `packages/common/src/utils.ts`'s `normalizeEOL` and `packages/common/src/font-metadata.ts`'s
//! `getLineHeight` at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`. Glyph widths come from
//! the caller's [`TextMeasure`], Excalidraw's `TextMetricsProvider`: `scene` has no fonts.

/// `DEFAULT_FONT_FAMILY` (`FONT_FAMILY.Excalifont`).
pub const DEFAULT_FONT_FAMILY: f64 = 5.0;
/// `DEFAULT_FONT_SIZE`.
pub const DEFAULT_FONT_SIZE: f64 = 20.0;

pub trait TextMeasure {
    /// `TextMetricsProvider.getLineWidth`: the width of `line`, which holds no line break, in
    /// scene units at `font_size`.
    fn line_width(&mut self, line: &str, font_family: f64, font_size: f64) -> f64;
}

/// `getLineHeight`: `FONT_METADATA[fontFamily].metrics.lineHeight`, Excalifont's for an id
/// `FONT_METADATA` has no entry for.
pub fn line_height(font_family: f64) -> f64 {
    if font_family == 2.0 || font_family == 7.0 || font_family == 9.0 {
        1.15
    } else if font_family == 3.0 {
        1.2
    } else {
        1.25
    }
}

/// `normalizeText`: `\r\n` and lone `\r` become `\n` (`normalizeEOL`), tabs become 8 spaces.
pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "        ")
}

/// `measureText` for already-normalized `text`: `[width, height]`, where width is the widest
/// line and height is `lines * fontSize * lineHeight`. An empty line measures as `" "`, as in
/// `measureText`.
pub fn measure_text(
    text: &str,
    font_family: f64,
    font_size: f64,
    line_height: f64,
    measure: &mut dyn TextMeasure,
) -> [f64; 2] {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| if line.is_empty() { " " } else { line })
        .collect();
    let height = font_size * line_height * lines.len() as f64;
    let width = lines
        .iter()
        .map(|line| measure.line_width(line, font_family, font_size))
        .fold(0.0, f64::max);
    [width, height]
}
```

`getTextWidth` 的初值是 0，逐行取 `Math.max`；寬度都是有限值，`f64::max` 與 `Math.max` 在這裡一致。

`sample.rs` 加：

```rust
/// The text metrics `tools/baseline/scene/generate.mjs` installs with
/// `setCustomTextMetricsProvider`: every UTF-16 code unit is `0.6 * fontSize` wide, whatever
/// the font.
pub struct CharWidthMeasure;

impl crate::text::TextMeasure for CharWidthMeasure {
    fn line_width(&mut self, line: &str, _font_family: f64, font_size: f64) -> f64 {
        line.encode_utf16().count() as f64 * font_size * 0.6
    }
}
```

Run: `cargo test -p scene text::tests`
Expected: PASS。

- [ ] **Step 4：JS 基準的 `newTextElement` case**

`tools/baseline/scene/cases.mjs` 的 `newElementCalls` 尾端加：

```js
  ["text/defaults", "newTextElement", { type: "text", id: "t1", seed: 13, x: 10, y: 20, text: "hello" }],
  ["text/centeredTwoLines", "newTextElement", {
    type: "text", id: "t2", seed: 14, x: 100, y: 50, text: "two\nlines", textAlign: "center",
    verticalAlign: "middle", fontSize: 28, fontFamily: 6, strokeColor: "#1971c2",
  }],
  ["text/emptyLine", "newTextElement", { type: "text", id: "t3", seed: 15, x: 0, y: 0, text: "a\n\nb", fontFamily: 8 }],
  ["text/crlfTabs", "newTextElement", {
    type: "text", id: "t4", seed: 16, x: 5, y: 5, text: "x\r\ny\tz", textAlign: "right", verticalAlign: "bottom",
  }],
  ["text/container", "newTextElement", {
    type: "text", id: "t5", seed: 17, x: 0, y: 0, text: "label", containerId: "r1", lineHeight: 1.5, fontFamily: 7,
  }],
  ["text/unknownFamily", "newTextElement", { type: "text", id: "t6", seed: 18, x: 0, y: 0, text: "?", fontFamily: 42 }],
```

`tools/baseline/scene/generate.mjs`：bundle 的 entry 加

```js
  export { newTextElement } from "@excalidraw/element/newElement";
  export { setCustomTextMetricsProvider } from "@excalidraw/element/textMeasurements";
```

（第一行併進既有的 `newElement` export 那行也可以。）在 `Date.now = () => 1;` 之前加：

```js
// Canvas text metrics do not exist in node; every UTF-16 code unit is 0.6em wide. The Rust
// side uses the same formula (`scene::sample::CharWidthMeasure`).
lib.setCustomTextMetricsProvider({ getLineWidth: (text, font) => text.length * parseFloat(font) * 0.6 });
```

Run: `cd tools/baseline && npm run scene`
Expected: `new_element: 14 cases (0 structure-only)`，其他群組的 JSON 不變（`git diff --stat` 只有 `new_element.json`）。

- [ ] **Step 5：Rust 基準測試接上 `newTextElement`，確認失敗**

`crates/scene/tests/baseline.rs`：`props_from` 的略過清單加上 `"text" | "fontSize" | "fontFamily" | "textAlign" | "verticalAlign" | "containerId" | "lineHeight"`；`new_element` 測試的 match 加一支：

```rust
            ("newTextElement", _) => new_text_element(
                props,
                TextProps {
                    text: opts["text"].as_str().expect("text").to_owned(),
                    font_size: opts.get("fontSize").map(num),
                    font_family: opts.get("fontFamily").map(num),
                    text_align: head("textAlign"),
                    vertical_align: head("verticalAlign"),
                    container_id: head("containerId"),
                    line_height: opts.get("lineHeight").map(num),
                },
                &mut CharWidthMeasure,
                env,
            ),
```

Run: `cargo test -p scene --test baseline new_element`
Expected: 編譯失敗（`new_text_element` 不存在）。

- [ ] **Step 6：實作 `new_text_element`**

照 `newTextElement` port：`fontFamily`、`fontSize`、`lineHeight` 用 JS 的 `||`（0 與 NaN 取預設，用 `rough::js::truthy`）；`textAlign`／`verticalAlign` 的 `||` 對空字串取預設 `"left"`／`"top"`；`text = normalizeText(opts.text)`；`measureText` 得到寬高；`x = opts.x - width * ratio_x`、`y = opts.y - height * ratio_y`（`getTextAnchorRatios`：`center` 0.5、`right` 1，`middle` 0.5、`bottom` 1，其他 0）。欄位：

```rust
    let (base, mut extra) = new_base(
        "text",
        ElementProps { x, y, width, height, ..props },
        env,
    );
    extra.insert("baseFontSize".into(), Value::Null);
    extra.insert("labelPosition".into(), Value::Null);
    Element::Text(TextElement {
        base,
        text: normalized.clone(),
        font_size,
        font_family,
        text_align,
        vertical_align,
        container_id: text.container_id.map_or(Slot::Null, Slot::Value),
        original_text: Some(normalized),
        auto_resize: Some(true),
        line_height: Some(line_height),
        extra,
    })
```

檔頭 doc comment 加上 `newTextElement`。在 `new_element.rs` 的 `tests` 模組加一個測試：`new_text_element` 的結果 `to_value()` 後再 `Element::from_value` 仍然是 `Element::Text`（型別化結構能無損寫回）。

Run: `cargo test -p scene --test baseline new_element && cargo test -p scene new_element`
Expected: PASS。

- [ ] **Step 7：`FontMeasure`**

`crates/app/src/render/text.rs` 加：

```rust
/// Widths for `scene::text::measure_text` from napkin's bundled fonts (and the system CJK
/// fallback), the way `TextMetricsProvider.getLineWidth` gets them from the canvas: one line
/// shaped without wrapping, its advance width.
pub struct FontMeasure {
    font_system: glyphon::FontSystem,
}

impl FontMeasure {
    pub fn new() -> FontMeasure {
        FontMeasure { font_system: font_system() }
    }
}

impl Default for FontMeasure {
    fn default() -> Self {
        FontMeasure::new()
    }
}

impl scene::text::TextMeasure for FontMeasure {
    fn line_width(&mut self, line: &str, font_family: f64, font_size: f64) -> f64 {
        if line.is_empty() || !(font_size > 0.0) {
            return 0.0;
        }
        let size = font_size as f32;
        let mut buffer =
            glyphon::Buffer::new(&mut self.font_system, glyphon::Metrics::new(size, size * 1.25));
        buffer.set_wrap(glyphon::Wrap::None);
        buffer.set_size(None, None);
        let attrs = glyphon::Attrs::new().family(glyphon::Family::Name(bundled_family(font_family)));
        buffer.set_text(line, &attrs, glyphon::Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
            .layout_runs()
            .map(|run| f64::from(run.line_w))
            .fold(0.0, f64::max)
    }
}
```

同檔 `tests` 模組加：

```rust
    #[test]
    fn font_measure_scales_with_size_and_glyphs() {
        use scene::text::TextMeasure;
        let mut measure = FontMeasure::new();
        let wide = measure.line_width("MMMM", 5.0, 20.0);
        let narrow = measure.line_width("iiii", 5.0, 20.0);
        assert!(wide > narrow && narrow > 0.0, "{wide} {narrow}");
        let double = measure.line_width("MMMM", 5.0, 40.0);
        assert!((double / wide - 2.0).abs() < 0.05, "{double} vs {wide}");
        assert!(measure.line_width("漢字", 5.0, 20.0) > 20.0, "system CJK fallback");
        assert_eq!(measure.line_width("", 5.0, 20.0), 0.0);
    }
```

Run: `cargo test -p app render::text`
Expected: PASS。

- [ ] **Step 8：全部檢查並 commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: 全部通過。

```bash
git add crates/scene/src/text.rs crates/scene/src/lib.rs crates/scene/src/new_element.rs \
  crates/scene/src/sample.rs crates/scene/tests/baseline.rs crates/scene/tests/baseline/new_element.json \
  crates/app/src/render/text.rs tools/baseline/scene/cases.mjs tools/baseline/scene/generate.mjs
git commit -m "Port Excalidraw text measurement and newTextElement"
```

---

### Task 2：skeleton 轉換、標籤與箭頭綁定

**Files:**
- Create: `crates/scene/src/binding.rs`、`crates/scene/src/batch/mod.rs`、`crates/scene/src/batch/add.rs`、`crates/scene/src/batch/validate.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/element.rs`、`crates/scene/src/transform.rs`、`crates/scene/src/editor/style.rs`、`crates/scene/src/editor/create.rs`、`crates/scene/tests/baseline.rs`
- Modify: `tools/baseline/lib/harness.mjs`、`tools/baseline/lib/excalidraw.mjs`、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`；新增 `crates/scene/tests/baseline/skeleton.json`

**Interfaces:**
- Consumes: Task 1 的 `text::*`、`new_element::{new_text_element, TextProps}`、`sample::CharWidthMeasure`；M4a 的 `new_element::{new_generic_element, new_line_element, new_arrow_element, new_freedraw_element, bump_version}`、`transform::bound_text_position`、`geometry::{element_absolute_coords, element_bounds, rotate_point, size_from_points}`、`fractional_index::sync_moved_indices`、`editor::ItemStyle`。
- Produces:

```rust
// scene::element
impl Element {
    /// Sets `startBinding`/`endBinding` to `binding` (typed line/arrow only; no-op otherwise).
    pub fn set_binding(&mut self, end: LinearEnd, binding: serde_json::Value);
    /// Appends `{id, type: kind}` to `boundElements` unless an entry with that id exists;
    /// `null` or missing becomes a one-entry array. Typed elements only.
    pub fn add_bound_element(&mut self, id: &str, kind: &str);
}

// scene::transform
/// `[getBoundTextMaxWidth, getBoundTextMaxHeight]` for a rectangle, diamond or ellipse.
pub fn bound_text_max_size(container: &Element) -> Option<[f64; 2]>;

// scene::binding
pub const BASE_BINDING_GAP: f64 = 5.0;
pub fn normalize_fixed_point(point: [f64; 2]) -> [f64; 2];
/// `getPointAtIndexGlobalCoordinates(arrow, 0 | -1)`.
pub fn linear_end_point(element: &Element, end: LinearEnd) -> Option<[f64; 2]>;
/// `calculateFixedPointForNonElbowArrowBinding` without a focus point.
pub fn fixed_point_for(arrow: &Element, end: LinearEnd, target: &Element) -> Option<[f64; 2]>;
/// `bindBindingElement(arrow, target, "orbit", end)` for a non-elbow arrow (`applyBinding`).
pub fn bind_arrow(file: &mut SceneFile, arrow: usize, end: LinearEnd, target: usize, env: &mut impl Env);

// scene::editor::ItemStyle (moved from editor/create.rs, now pub(crate))
impl ItemStyle {
    pub(crate) fn generic_roundness(&self, kind: GenericKind) -> Option<Roundness>;
    pub(crate) fn line_roundness(&self) -> Option<Roundness>;
    pub(crate) fn arrow_roundness(&self) -> Option<Roundness>;
    pub(crate) fn props(&self, origin: [f64; 2], width: f64, height: f64,
                        roundness: Option<Roundness>, stroke_width: f64) -> ElementProps;
}

// scene::batch
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OpError {
    /// Position of the offending op in `ops`; `None` for an error about the batch as a whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<usize>,
    /// JSON path inside that op, e.g. `"label.fontSize"` or `"points[2]"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

// scene::batch::add
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Added {
    /// Skeleton `id` -> generated id, for every skeleton that gave one.
    pub aliases: BTreeMap<String, String>,
    /// Generated ids of the skeletons, in input order (labels not included).
    pub ids: Vec<String>,
    /// Labels that do not fit their container.
    pub warnings: Vec<String>,
}
/// `LabelSpec`: a skeleton's `label`, or `update`'s `text` on a container without one.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelSpec {
    pub text: String,
    pub font_size: Option<f64>,
    pub font_family: Option<f64>,
    pub text_align: Option<String>,
    pub vertical_align: Option<String>,
}
/// `convertToExcalidrawElements` for napkin's subset, appended to `file`. `skeletons` pairs
/// each skeleton with its op position, for error positions. Validates everything first and
/// changes nothing on error.
pub fn add_elements(file: &mut SceneFile, skeletons: &[(usize, &Value)], style: &ItemStyle,
                    measure: &mut dyn TextMeasure, env: &mut impl Env) -> Result<Added, Vec<OpError>>;
/// `bindTextToContainer` + `redrawTextBoundingBox` without wrapping or growing the container:
/// appends a label to the container at `container`, returns its position, and pushes a warning
/// when it does not fit (`name` is how the warning refers to the container).
pub(crate) fn bind_label(file: &mut SceneFile, container: usize, label: &LabelSpec, name: &str,
                         measure: &mut dyn TextMeasure, env: &mut impl Env,
                         warnings: &mut Vec<String>) -> usize;
/// First point moved to `[0, 0]`, the offset added to the origin (`getNormalizedPoints`).
pub(crate) fn normalize_points(origin: [f64; 2], points: &[[f64; 2]]) -> ([f64; 2], Vec<[f64; 2]>);
```

要讀的 JS：`packages/element/src/transform.ts` 全檔（`convertToExcalidrawElements`、`bindTextToContainer`、`bindLinearElementToElement`）；`packages/element/src/binding.ts` 的 `bindBindingElement`、`applyBinding`、`calculateFixedPointForNonElbowArrowBinding`、`getBindingGap`、`normalizeFixedPoint`、`MIN_BINDABLE_SIZE`；`packages/element/src/linearElementEditor.ts` 的 `getPointAtIndexGlobalCoordinates`、`getNormalizedPoints`、`getNormalizeElementPointsAndCoords`；`packages/element/src/bounds.ts` 的 `elementCenterPoint`、`getCenterForBounds`；`packages/element/src/textElement.ts` 的 `redrawTextBoundingBox`；`packages/element/src/mutateElement.ts` 的 `mutateElement`。

- [ ] **Step 1：harness 與 stub**

`tools/baseline/lib/harness.mjs` 的 `runCase` 加第三個參數：

```js
/**
 * ...(keep the existing paragraph)...
 *
 * With `{ exactDespiteRandom: true }`, a case that calls Math.random but produces byte-identical
 * output under both streams (the caller already stripped every random-derived value) is still
 * compared number by number.
 */
export function runCase(name, fn, { exactDespiteRandom = false } = {}) {
  randomCalls = 0;
  randomStream = lcg(1);
  const first = encode(capture(fn));
  if (randomCalls === 0) {
    return { compare: "exact", expected: first };
  }
  randomStream = lcg(2);
  const second = encode(capture(fn));
  if (exactDespiteRandom) {
    if (JSON.stringify(first) !== JSON.stringify(second)) {
      throw new Error(`${name}: output still depends on Math.random after normalization`);
    }
    return { compare: "exact", expected: first };
  }
  if (JSON.stringify(structureOf(first)) !== JSON.stringify(structureOf(second))) {
    throw new Error(`${name}: output structure depends on Math.random; drop this case`);
  }
  return { compare: "structure", expected: structureOf(first) };
}
```

`tools/baseline/lib/excalidraw.mjs` 的 stub：`nanoid` 改成遞增計數器，其他不變（既有群組從未呼叫它，否則早就丟例外了）：

```js
      contents: [
        "const unused = () => { throw new Error('stubbed module called'); };",
        "let nanoidCount = 0;",
        "export default () => unused; export const sanitizeUrl = unused;",
        "export const nanoid = () => `nanoid-${++nanoidCount}`;",
      ].join("\n"),
```

並把上方註解改成說明 `nanoid` 回傳計數器字串（skeleton 轉換一定會產生 id）。

- [ ] **Step 2：skeleton 基準 case**

`tools/baseline/scene/cases.mjs` 尾端加：

```js
/** [case name, skeleton list] for convertToExcalidrawElements. Every label fits its container
 * under the generator's 0.6em text metrics: napkin never wraps or grows (AI spec §4.5). */
export const skeletonBatches = [
  ["shapes", [
    { type: "rectangle", x: 0, y: 0, width: 120, height: 60, strokeColor: "#1971c2", backgroundColor: "#a5d8ff" },
    { type: "diamond", x: 200, y: 0, width: 100, height: 80, fillStyle: "hachure", strokeWidth: 4, strokeStyle: "dashed" },
    { type: "ellipse", x: 0, y: 150, width: 90, height: 40, roughness: 0, opacity: 60, groupIds: ["g1"] },
  ]],
  ["labels", [
    { type: "rectangle", id: "r", x: 0, y: 0, width: 160, height: 70, label: { text: "Parser" } },
    { type: "diamond", id: "d", x: 200, y: 0, width: 200, height: 160, strokeColor: "#e03131",
      label: { text: "a\nb", textAlign: "left", verticalAlign: "top", fontSize: 16 } },
    { type: "ellipse", id: "e", x: 0, y: 200, width: 220, height: 120, label: { text: "Nunito", fontFamily: 6 } },
  ]],
  ["text", [
    { type: "text", x: 10, y: 10, text: "left" },
    { type: "text", x: 200, y: 10, text: "centered", textAlign: "center" },
    { type: "text", x: 0, y: 100, text: "code\nblock", fontFamily: 8, fontSize: 28, strokeColor: "#2f9e44" },
  ]],
  ["lines", [
    { type: "line", x: 0, y: 0, width: 100, height: 50 },
    { type: "line", x: 0, y: 100, width: 80, height: -40 },
    { type: "arrow", x: 0, y: 200 },
    { type: "arrow", x: 0, y: 300, points: [[0, 0], [60, 40], [120, 0]] },
    { type: "arrow", x: 300, y: 0, points: [[0, 0], [-100, 0]] },
    { type: "arrow", x: 300, y: 100, points: [[0, 0], [0, -80]], startArrowhead: "dot", endArrowhead: "triangle" },
  ]],
  ["bindings", [
    { type: "arrow", id: "a", x: 125, y: 30, points: [[0, 0], [70, 0]], start: { id: "r1" }, end: { id: "r2" } },
    { type: "rectangle", id: "r1", x: 0, y: 0, width: 120, height: 60, label: { text: "one" } },
    { type: "rectangle", id: "r2", x: 200, y: 0, width: 120, height: 60 },
    { type: "arrow", id: "self", x: 60, y: 65, points: [[0, 0], [0, 40], [260, 40], [260, 0]], start: { id: "r1" }, end: { id: "r2" } },
    { type: "arrow", id: "inside", x: 30, y: 30, points: [[0, 0], [200, 0]], start: { id: "r1" }, end: { id: "r1" } },
  ]],
  ["tinyTarget", [
    { type: "ellipse", id: "dot", x: 0, y: 0, width: 0.5, height: 20 },
    { type: "arrow", x: 50, y: 10, points: [[0, 0], [-45, 0]], end: { id: "dot" } },
  ]],
];
```

`tools/baseline/scene/generate.mjs`：import `skeletonBatches`；bundle entry 加 `export { convertToExcalidrawElements } from "@excalidraw/element/transform";`；在檔尾（`Date.now` 已經釘在 1 之後）加：

```js
/**
 * Stable across Math.random streams: `seed` and `versionNonce` are dropped and every id is
 * renamed to `e<position>` in output order, references included. `crates/scene/tests/
 * baseline.rs`'s `normalize_skeleton_output` does the same.
 */
function normalizeSkeletonOutput(elements) {
  const rename = new Map(elements.map((e, i) => [e.id, `e${i}`]));
  const id = (value) => rename.get(value) ?? value;
  return elements.map(({ seed, versionNonce, ...rest }) => {
    const out = structuredClone(rest);
    out.id = id(out.id);
    if (typeof out.containerId === "string") out.containerId = id(out.containerId);
    if (Array.isArray(out.boundElements)) out.boundElements = out.boundElements.map((b) => ({ ...b, id: id(b.id) }));
    for (const key of ["startBinding", "endBinding"]) {
      if (out[key]) out[key] = { ...out[key], elementId: id(out[key].elementId) };
    }
    return out;
  });
}

writeGroup(
  outDir,
  "skeleton",
  source,
  skeletonBatches.map(([name, skeletons]) => ({
    name,
    call: "convertToExcalidrawElements",
    args: [skeletons],
    ...runCase(
      name,
      () => normalizeSkeletonOutput(lib.convertToExcalidrawElements(structuredClone(skeletons))),
      { exactDespiteRandom: true },
    ),
  })),
);
```

Run: `cd tools/baseline && npm run scene`
Expected: `skeleton: 6 cases (0 structure-only)`；其他群組的 JSON 不變。打開 `skeleton.json` 確認每個 label 的 `width` 小於容器寬減 10、箭頭有 `startBinding.fixedPoint`、`version` 有大於 1 的元件。任何一個 label 放不下（容器寬被撐大）就調整該 case 的容器尺寸再重新產生。

- [ ] **Step 3：Rust 基準測試（先失敗）**

`crates/scene/tests/baseline.rs` 加：

```rust
/// `generate.mjs`'s `normalizeSkeletonOutput`: drops `seed` and `versionNonce`, renames ids to
/// `e<position>` in order, references included.
fn normalize_skeleton_output(elements: &[Element]) -> Value {
    let values: Vec<Value> = elements.iter().map(Element::to_value).collect();
    let rename: HashMap<String, String> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (v["id"].as_str().expect("id").to_owned(), format!("e{i}")))
        .collect();
    let id = |v: &Value| json!(rename.get(v.as_str().unwrap_or("")).cloned().unwrap_or_default());
    Value::Array(
        values
            .into_iter()
            .map(|mut v| {
                let map = v.as_object_mut().expect("object");
                map.remove("seed");
                map.remove("versionNonce");
                map["id"] = id(&map["id"]);
                if map.get("containerId").is_some_and(Value::is_string) {
                    map["containerId"] = id(&map["containerId"]);
                }
                if let Some(Value::Array(bound)) = map.get_mut("boundElements") {
                    for b in bound {
                        b["id"] = id(&b["id"]);
                    }
                }
                for key in ["startBinding", "endBinding"] {
                    if let Some(binding) = map.get_mut(key).filter(|b| b.is_object()) {
                        binding["elementId"] = id(&binding["elementId"]);
                    }
                }
                v
            })
            .collect(),
    )
}

#[test]
fn skeleton() {
    let style = ItemStyle {
        edges: EdgeStyle::Sharp,
        arrow_type: ArrowType::Sharp,
        ..ItemStyle::default()
    };
    check_group(&dir(), "skeleton", |case| {
        let skeletons = case.args[0].as_array().expect("skeletons");
        let pairs: Vec<(usize, &Value)> = skeletons.iter().enumerate().collect();
        let mut file = scene::SceneFile::new();
        add_elements(&mut file, &pairs, &style, &mut CharWidthMeasure, &mut FixedEnv)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        normalize_skeleton_output(&file.elements)
    });
}
```

（`use std::collections::HashMap;`、`scene::batch::add_elements`、`scene::editor::{ArrowType, EdgeStyle, ItemStyle}`、`scene::sample::CharWidthMeasure`。）

Run: `cargo test -p scene --test baseline skeleton`
Expected: 編譯失敗。

- [ ] **Step 4：`element.rs` 的新方法、`bound_text_max_size`、`ItemStyle` 輔助函數**

`element.rs` 加 `set_binding` 與 `add_bound_element`，照既有 `clear_binding` 的寫法（寫進 `extra_mut()`，`Raw` 不動），並在 `tests` 加：

```rust
    #[test]
    fn binding_and_bound_element_setters() {
        let mut arrow = Element::from_value(crate::sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]));
        let binding = json!({"elementId": "r", "mode": "orbit", "fixedPoint": [1.0, 0.5001]});
        arrow.set_binding(LinearEnd::End, binding.clone());
        assert_eq!(arrow.to_value()["endBinding"], binding);
        assert_eq!(arrow.binding_target(LinearEnd::End), Some("r"));

        let mut rect = Element::from_value(rectangle());
        rect.add_bound_element("a", "arrow");
        rect.add_bound_element("a", "arrow");
        rect.add_bound_element("t", "text");
        assert_eq!(rect.bound_elements(), vec![("a", "arrow"), ("t", "text")]);

        let mut image = Element::from_value(json!({"id": "i", "type": "image"}));
        image.add_bound_element("a", "arrow");
        assert_eq!(image.to_value(), json!({"id": "i", "type": "image"}));
    }
```

`transform.rs`：把 `bound_text_position` 裡的 `getBoundTextMaxWidth`／`getBoundTextMaxHeight` 那段抽成 `pub fn bound_text_max_size(container: &Element) -> Option<[f64; 2]>`（非 rectangle／diamond／ellipse 回 `None`），`bound_text_position` 改呼叫它。行為不變，既有測試照樣通過。

`editor/style.rs`：把 `create.rs` 的 `round_proportional`、`generic_roundness`、`item_props` 搬過來，改成 `ItemStyle` 的 `pub(crate)` 方法 `generic_roundness(kind)`、`line_roundness()`（`edges == Round` 時 `{type: 2}`）、`arrow_roundness()`（`arrow_type == Round` 時 `{type: 2}`）、`props(origin, width, height, roundness, stroke_width)`；`create.rs` 改用它們。行為不變。

Run: `cargo test -p scene`
Expected: 除了 `skeleton` 基準（仍編譯失敗的話先註解掉該測試）之外全部通過。

- [ ] **Step 5：`binding.rs`**

```rust
//! Arrow binding creation, ported at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` from
//! `packages/element/src/binding.ts`'s `bindBindingElement` (non-elbow branch), `applyBinding`,
//! `calculateFixedPointForNonElbowArrowBinding` (no focus point), `getBindingGap` and
//! `normalizeFixedPoint`; `packages/element/src/linearElementEditor.ts`'s
//! `getPointAtIndexGlobalCoordinates`; and `packages/element/src/bounds.ts`'s
//! `elementCenterPoint`/`getCenterForBounds`. Arrows following a moved shape
//! (`updateBoundElements`) is not here: napkin does not do it yet.

use serde_json::json;

use crate::collision;
use crate::element::{Element, LinearEnd};
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::{element_absolute_coords, element_bounds, rotate_point};
use crate::new_element::bump_version;

/// `BASE_BINDING_GAP`.
pub const BASE_BINDING_GAP: f64 = 5.0;
/// `MIN_BINDABLE_SIZE`.
const MIN_BINDABLE_SIZE: f64 = 1.0;
/// `FIXED_POINT_BOUND`.
const FIXED_POINT_BOUND: f64 = 10.0;

pub fn normalize_fixed_point(point: [f64; 2]) -> [f64; 2] {
    if !point[0].is_finite() || !point[1].is_finite() {
        return [0.5001, 0.5001];
    }
    const EPSILON: f64 = 0.0001;
    let clamped = point.map(|ratio| ratio.clamp(-FIXED_POINT_BOUND, FIXED_POINT_BOUND));
    if clamped.iter().any(|ratio| (ratio - 0.5).abs() < EPSILON) {
        clamped.map(|ratio| if (ratio - 0.5).abs() < EPSILON { 0.5001 } else { ratio })
    } else {
        clamped
    }
}

pub fn linear_end_point(element: &Element, end: LinearEnd) -> Option<[f64; 2]> {
    let (Element::Line(l) | Element::Arrow(l)) = element else {
        return None;
    };
    let point = match end {
        LinearEnd::Start => l.points.first(),
        LinearEnd::End => l.points.last(),
    }?;
    let (_, center) = element_absolute_coords(element)?;
    Some(rotate_point(
        [l.base.x + point[0], l.base.y + point[1]],
        center,
        l.base.angle,
    ))
}

pub fn fixed_point_for(arrow: &Element, end: LinearEnd, target: &Element) -> Option<[f64; 2]> {
    let edge = linear_end_point(arrow, end)?;
    let placement = target.placement()?;
    let [x1, y1, x2, y2] = element_bounds(target)?;
    let center = [x1 + (x2 - x1) / 2.0, y1 + (y2 - y1) / 2.0];
    let unrotated = rotate_point(edge, center, -placement.angle);
    if placement.width < MIN_BINDABLE_SIZE || placement.height < MIN_BINDABLE_SIZE {
        return Some(normalize_fixed_point([0.5, 0.5]));
    }
    // `getBindingGap` for a non-elbow arrow.
    let gap = BASE_BINDING_GAP + collision::stroke_width(target) / 2.0;
    Some(normalize_fixed_point([
        (unrotated[0] - placement.x) / placement.width.max(gap),
        (unrotated[1] - placement.y) / placement.height.max(gap),
    ]))
}

/// Sets the arrow's binding to `{elementId, mode: "orbit", fixedPoint}` and, unless the target
/// already lists the arrow, appends it to the target's `boundElements`; each element that
/// changed gets a version bump (`scene.mutateElement` in `applyBinding`).
pub fn bind_arrow(
    file: &mut SceneFile,
    arrow: usize,
    end: LinearEnd,
    target: usize,
    env: &mut impl Env,
) {
    let Some(fixed_point) = fixed_point_for(&file.elements[arrow], end, &file.elements[target])
    else {
        return;
    };
    let target_id = file.elements[target].id().expect("bindable target has an id").to_owned();
    let arrow_id = file.elements[arrow].id().expect("arrow has an id").to_owned();
    let before = file.elements[arrow].clone();
    file.elements[arrow].set_binding(
        end,
        json!({"elementId": target_id, "mode": "orbit", "fixedPoint": fixed_point}),
    );
    if file.elements[arrow] != before {
        bump_version(&mut file.elements[arrow], env);
    }
    let before = file.elements[target].clone();
    file.elements[target].add_bound_element(&arrow_id, "arrow");
    if file.elements[target] != before {
        bump_version(&mut file.elements[target], env);
    }
}
```

`collision::stroke_width` 目前是 `pub(crate)`，同 crate 可用。`binding.rs` 的 `tests` 至少涵蓋：`normalize_fixed_point` 的 NaN、夾限到 ±10、0.5 附近改 0.5001；旋轉 90° 的目標上 `fixed_point_for` 算出的比例（手算）；目標寬小於 1 時回 `[0.5001, 0.5001]`；`bind_arrow` 對同一個目標綁兩端時 `boundElements` 只有一筆。

- [ ] **Step 6：`batch/validate.rs`**

欄位驗證的小函數，全部回 `Result<T, String>`（錯誤訊息，不含欄位名稱；欄位名稱由呼叫端放進 `OpError::field`）：

```rust
pub(crate) fn finite(value: &Value) -> Result<f64, String>;            // "must be a finite number"
pub(crate) fn positive(value: &Value) -> Result<f64, String>;          // "must be a number greater than 0"
pub(crate) fn string(value: &Value) -> Result<String, String>;         // "must be a string"
pub(crate) fn non_empty_string(value: &Value) -> Result<String, String>;
pub(crate) fn one_of(value: &Value, allowed: &[&str]) -> Result<String, String>; // "must be one of: a, b"
pub(crate) fn font_family(value: &Value) -> Result<f64, String>;       // 1, 2, 3, 5, 6, 7, 8, 9, 10
pub(crate) fn opacity(value: &Value) -> Result<f64, String>;           // 0..=100
pub(crate) fn roughness(value: &Value) -> Result<f64, String>;         // finite, >= 0
pub(crate) fn group_ids(value: &Value) -> Result<Vec<String>, String>; // array of strings
/// An array of `[x, y]` finite pairs with at least `min` entries; `Err((index, message))` names
/// the bad entry (`None` for the array itself).
pub(crate) fn points(value: &Value, min: usize) -> Result<Vec<[f64; 2]>, (Option<usize>, String)>;

pub(crate) const FILL_STYLES: &[&str] = &["hachure", "cross-hatch", "solid", "zigzag"];
pub(crate) const STROKE_STYLES: &[&str] = &["solid", "dashed", "dotted"];
pub(crate) const TEXT_ALIGNS: &[&str] = &["left", "center", "right"];
pub(crate) const VERTICAL_ALIGNS: &[&str] = &["top", "middle", "bottom"];
/// `Arrowhead` and `ArrowheadLegacy` (`packages/element/src/types.ts`).
pub(crate) const ARROWHEADS: &[&str] = &[
    "arrow", "bar", "circle", "circle_outline", "triangle", "triangle_outline", "diamond",
    "diamond_outline", "cardinality_one", "cardinality_many", "cardinality_one_or_many",
    "cardinality_exactly_one", "cardinality_zero_or_one", "cardinality_zero_or_many", "dot",
    "crowfoot_one", "crowfoot_many", "crowfoot_one_or_many",
];

/// The style keys every element type accepts, in `add` and `update`.
pub(crate) const STYLE_KEYS: &[&str] = &[
    "strokeColor", "backgroundColor", "fillStyle", "strokeWidth", "strokeStyle", "roughness", "opacity",
];
```

`tests` 各給一個接受與拒絕的例子即可。

- [ ] **Step 7：`batch/add.rs`**

`batch/mod.rs` 先放：

```rust
//! One `napkin apply` batch (AI spec §4): parsing, the `add` skeleton conversion, `update` and
//! `delete`, applied all-or-nothing.

mod add;
mod validate;

pub use add::{Added, LabelSpec, add_elements};
// OpError as in Interfaces above.
```

`add_elements` 的流程（對照 `convertToExcalidrawElements`，每一步都去讀對應的 JS）：

1. **驗證**（不動 `file`）：每個 skeleton 必須是物件，`type` 是 `rectangle`／`diamond`／`ellipse`／`text`／`line`／`arrow`／`freedraw` 之一（`image`、`frame` 等回 `"unsupported type \"image\"; napkin can add rectangle, diamond, ellipse, text, line, arrow and freedraw"`）。允許的 key：共用 `op`（忽略）、`type`、`id`、`x`、`y`、`groupIds` 與 `STYLE_KEYS`；形狀加 `width`、`height`、`label`；`text` 加 `text`、`fontSize`、`fontFamily`、`textAlign`；`line` 加 `points`、`width`、`height`；`arrow` 再加 `start`、`end`、`startArrowhead`、`endArrowhead`；`freedraw` 加 `points`。其他 key 回 `"unknown field"`；線或箭頭帶 `label` 回 `"napkin does not create labels on lines or arrows"`。`x`、`y` 必填。形狀的 `width`、`height` 必填且大於 0。`label.text` 必須是非空字串，`label` 其他 key 只能是 `fontSize`、`fontFamily`、`textAlign`、`verticalAlign`。`start`／`end` 只能是 `{"id": 字串}`；帶 `type` 等其他 key 回 `"start/end can only name an existing element by id; add the shape first"`。`points` 線與箭頭至少 2 點、手繪至少 1 點。`id`（代號）重複回錯誤。`start`／`end` 的 id 先找同一批的代號，再找 `file` 裡未刪除的元件；都找不到回 `"no element \"x\""`；找到的必須是型別化的 rectangle／diamond／ellipse，否則 `"arrows can only bind to rectangles, diamonds and ellipses"`。有任何錯誤就回 `Err`，`file` 不動。
2. **建立**，依輸入順序，每個 append 到 `file.elements`（`index` 先保持 null）：
   - 形狀：`new_generic_element(kind, style.props([x, y], width, height, style.generic_roundness(kind), style.stroke_width.value(false)) + 覆寫 skeleton 給的樣式與 groupIds, env)`。
   - `text`：`new_text_element(ElementProps{ x, y, 樣式, .. }, TextProps{ text, font_size, font_family, text_align, .. }, measure, env)`。`x` 是 `textAlign` 對應的錨點（`newTextElement` 的 offsets）。
   - `line`：沒給 `points` 時 `[[0, 0], [w, h]]`，`w`／`h` 有給就用（包括 0），否則 100 與 0；給了就 `normalize_points`；`width`／`height` 用 `size_from_points`（與 spec 不同的地方 3、4、5）。`new_line_element`，roundness 用 `style.line_roundness()`。
   - `arrow`：點的處理同上；`width`／`height` 用 `size_from_points`；`new_arrow_element(props, points, startArrowhead 或 style.start_arrowhead, endArrowhead 或 style.end_arrowhead（skeleton 沒給時是 `"arrow"`，照 JS 的 `endArrowhead: "arrow"` 預設）, env)`，roundness `style.arrow_roundness()`。
   - `freedraw`：`normalize_points`、`size_from_points`，`new_freedraw_element(props, points, vec![], true, Some(StrokeOptions{ variability: style.stroke_variability, streamline: 0.5 }), env)`，`strokeWidth` 預設 `style.stroke_width.value(true)`。
   - 有代號的記進 `aliases`，全部的 id 記進 `ids`。
3. **標籤與綁定**：再照輸入順序走一次（JS 的第二個迴圈，順序決定 `boundElements` 的排列）：形狀有 `label` 就 `bind_label`；箭頭有 `start`／`end` 就 `bind_arrow`（先 start 再 end），然後做 `bindLinearElementToElement` 尾段的 0.5 位移與 `getNormalizeElementPointsAndCoords`（所有箭頭都做，不論有沒有綁定；只改 `points`、`x`、`y`，不改 `width`／`height`，也不 bump，對應 JS 的 `Object.assign`）。
4. **index**：`fractional_index::sync_moved_indices(&mut file.elements, &這批新元件（含標籤）的 id 集合, env)`。

`bind_label` 對照 `bindTextToContainer` 與 `redrawTextBoundingBox`：`new_text_element(ElementProps{ x: 0, y: 0, stroke_color: 容器的, ..ElementProps::default() }, TextProps{ text, font_size, font_family, text_align: Some(align.unwrap_or("center")), vertical_align: Some(valign.unwrap_or("middle")), container_id: Some(容器 id), line_height: None }, measure, env)`；label 的 `angle` 設成容器的 `angle`；push 到尾端；容器 `add_bound_element(label_id, "text")` 不 bump（JS 是 `Object.assign`）；再用 `bound_text_position` 算位置、寫入 `x`／`y`，有變就 bump label（JS 的 `scene.mutateElement(textElement, boundTextUpdates)`）。不換行、不撐大容器；`bound_text_max_size` 比 label 小時 push 警告：

```rust
format!(
    "{name}: label needs {}x{} but the {} fits {}x{}; make the {} larger or add line breaks",
    fmt(width), fmt(height), kind, fmt(max_w), fmt(max_h), kind
)
```

`fmt` 四捨五入到整數即可（在 `add.rs` 內部定義）。`name` 是代號，沒有代號就是真正的 id。

- [ ] **Step 8：`add.rs` 的單元測試**

`batch/add.rs` 的 `tests`（用 `sample::CharWidthMeasure` 與一個固定的 `Env`）至少涵蓋：

```rust
    #[test]
    fn rejects_the_whole_list_and_leaves_the_file_alone() {
        let mut file = sample::file(vec![sample::generic("rectangle", "keep", [0.0, 0.0, 10.0, 10.0])]);
        let before = file.clone();
        let ops = [
            json!({"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}),
            json!({"type": "image", "x": 0, "y": 0}),
            json!({"type": "arrow", "x": 0, "y": 0, "label": {"text": "no"}}),
            json!({"type": "arrow", "x": 0, "y": 0, "end": {"type": "rectangle"}}),
            json!({"type": "rectangle", "x": 0, "y": 0, "width": 0, "height": 10}),
            json!({"type": "arrow", "x": 0, "y": 0, "start": {"id": "missing"}}),
        ];
        let pairs: Vec<(usize, &Value)> = ops.iter().enumerate().collect();
        let errors = add_elements(&mut file, &pairs, &ItemStyle::default(), &mut CharWidthMeasure, &mut env())
            .unwrap_err();
        let positions: Vec<Option<usize>> = errors.iter().map(|e| e.op).collect();
        assert_eq!(positions, vec![Some(1), Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!(errors[3].field.as_deref(), Some("width"));
        assert_eq!(file, before);
    }
```

另外：代號對應與 `ids` 順序；箭頭用代號指向排在後面的形狀；指向 `file` 裡既有的矩形時，那個矩形 `boundElements` 多一筆且 `version` 加 1；指向既有的 `Raw` 元件或文字回錯誤；`width: 0, height: 80` 沒給 `points` 的箭頭是垂直的（與 spec 不同的地方 5）；`points` 從 `[10, 10]` 開始的線被正規化、`x`／`y` 加 10；標籤放不下時有警告、容器尺寸不變；`ItemStyle::default()` 建出的矩形 `roundness` 是 `{type: 3}`；新元件的 `index` 排在既有元件之後。

Run: `cargo test -p scene batch && cargo test -p scene --test baseline skeleton`
Expected: PASS。基準不過時，照差異讀 JS 修 Rust；不要改基準。

- [ ] **Step 9：全部檢查並 commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && (cd tools/baseline && npm run scene) && git status --short crates/scene/tests/baseline`
Expected: 全部通過；重新產生後除了新加的 `skeleton.json` 之外沒有差異。

```bash
git add crates/scene tools/baseline
git commit -m "Port Excalidraw skeleton conversion with labels and arrow bindings"
```

---

### Task 3：一批操作：解析、`update`、`delete`

**Files:**
- Modify: `crates/scene/src/batch/mod.rs`
- Test: 同檔的 `tests` 模組

**Interfaces:**
- Consumes: Task 2 的 `add_elements`、`bind_label`、`normalize_points`、`LabelSpec`、`OpError`、`validate::*`、`binding::{bind_arrow, fixed_point_for}`；Task 1 的 `text::{measure_text, normalize_text}`；M4a 的 `edit::delete_selection`、`transform::{bound_text_position, bound_text_max_size}`、`selection::Selection`。
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatchReport {
    /// `add` aliases -> generated ids.
    pub created: BTreeMap<String, String>,
    /// Ids of the elements the `add` ops created, in op order (labels not included).
    pub added: Vec<String>,
    /// Ids named by `update` ops, first mention order, deduplicated.
    pub updated: Vec<String>,
    /// Ids named by `delete` ops, deduplicated.
    pub deleted: Vec<String>,
    pub warnings: Vec<String>,
}

/// Applies `batch` (`{"ops": [...]}`) to a copy of `file`: every `add` first (one
/// `add_elements` call), then `update` and `delete` in op order. Returns the new scene only when
/// every op succeeded; otherwise every error found, and `file` is untouched by construction.
pub fn apply_batch(file: &SceneFile, batch: &Value, style: &ItemStyle,
                   measure: &mut dyn TextMeasure, env: &mut impl Env)
    -> Result<(SceneFile, BatchReport), Vec<OpError>>;
```

`update` 的規則（AI spec §4.3 加上與 spec 不同的地方 8）：

| 元件 | 可改 |
|---|---|
| 型別化 rectangle／diamond／ellipse | `x`、`y`、`width`（>0）、`height`（>0）、`text`（標籤）、`STYLE_KEYS` |
| 沒綁定的 text | `x`、`y`、`text`、`STYLE_KEYS` |
| 綁定在容器上的 text | `text`、`STYLE_KEYS`（`x`／`y` 回 `"a label follows its container; move the container instead"`） |
| line／arrow | `x`、`y`、`points`（≥2）、`STYLE_KEYS`（`text` 回 `"napkin does not create labels on lines or arrows"`） |
| freedraw | `x`、`y`、`STYLE_KEYS` |
| `Raw` | `x`、`y`（其他回 `"<id> is a <type> napkin can only move"`） |

修改之後的連帶處理：

- 容器的位置或大小變了，或它的標籤文字變了：標籤用 `bound_text_position` 重新定位。
- 容器 `text`：有活著的標籤就改它的 `text` 與 `originalText`（先 `normalize_text`）、用 `measure_text` 重算寬高；沒有就 `bind_label` 建一個（新 id 不列進 `added`）。`text` 為空字串回 `"text must not be empty"`。
- 沒綁定的 text 改 `text`：重算寬高，`x`／`y` 不動。
- 容器的 `strokeColor` 同時套到它的標籤（Excalidraw 的顏色動作包含綁定文字）。
- 線或箭頭改 `points`：`normalize_points`，`width`／`height` 用 `size_from_points`。
- 箭頭改 `x`／`y`：它自己的標籤（`boundElements` 裡的 text）跟著平移同樣的量，照 M4a 的搬移。
- 箭頭改了位置或點：每個有綁定的端點用 `fixed_point_for` 重算並 `set_binding`（`mode` 保留原值，沒有就 `"orbit"`）。
- 形狀改了位置或大小：`boundElements` 裡每支活著的箭頭，綁在這個形狀的那一端重算 `fixedPoint`。
- 標籤放不下時同 Task 2 的警告。
- 每個被改的元件只在真的變了時 bump 一次（先 clone，改完比較）。

`delete`：`{"op": "delete", "ids": [...]}`，`ids` 非空、每個都要存在且未刪除（可以是同一批 add 的代號），最後呼叫一次 `edit::delete_selection(file, &Selection::from_ids(ids), env)`。

- [ ] **Step 1：失敗的測試**

`batch/mod.rs` 的 `tests`：

```rust
    use serde_json::json;

    use super::*;
    use crate::sample::{self, CharWidthMeasure};

    fn scene() -> SceneFile {
        sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 60.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}]}),
            ),
            sample::text("t", [26.0, 17.5, 48.0, 25.0], "box", Some("r")),
            sample::with(
                sample::linear("arrow", "a", [105.0, 30.0], &[[0.0, 0.0], [90.0, 0.0]]),
                json!({"startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [1.05, 0.5001]}}),
            ),
            json!({"id": "img", "type": "image", "x": 300, "y": 0, "width": 50, "height": 50,
                   "isDeleted": false, "version": 1, "versionNonce": 1}),
        ])
    }

    fn apply(file: &SceneFile, batch: Value) -> Result<(SceneFile, BatchReport), Vec<OpError>> {
        apply_batch(file, &batch, &ItemStyle::default(), &mut CharWidthMeasure, &mut TestEnv(0))
    }

    fn get<'a>(file: &'a SceneFile, id: &str) -> &'a Element {
        file.elements.iter().find(|e| e.id() == Some(id)).expect(id)
    }

    #[test]
    fn a_failing_op_rejects_the_whole_batch() {
        let file = scene();
        let errors = apply(&file, json!({"ops": [
            {"op": "update", "id": "r", "set": {"x": 10}},
            {"op": "update", "id": "gone", "set": {"x": 1}},
            {"op": "update", "id": "img", "set": {"strokeColor": "#e03131"}},
            {"op": "delete", "ids": []},
            {"op": "paint"},
        ]})).unwrap_err();
        let positions: Vec<Option<usize>> = errors.iter().map(|e| e.op).collect();
        assert_eq!(positions, vec![Some(1), Some(2), Some(3), Some(4)]);
        assert!(apply(&file, json!({"nope": []})).unwrap_err()[0].op.is_none());
        assert!(apply(&file, json!({"ops": []})).is_err(), "an empty batch is an error");
    }

    #[test]
    fn moving_a_container_recenters_its_label_and_refreshes_arrow_bindings() {
        let (next, report) = apply(&scene(), json!({"ops": [
            {"op": "update", "id": "r", "set": {"x": 0, "y": 100, "width": 200}},
        ]})).unwrap();
        assert_eq!(report.updated, vec!["r"]);
        let label = get(&next, "t").placement().unwrap();
        assert_eq!((label.x, label.y), (76.0, 117.5));
        // The arrow stayed where it was; its start's fixedPoint now describes (105, 30)
        // relative to the moved, wider rectangle.
        let binding = get(&next, "a").to_value()["startBinding"].clone();
        assert_eq!(binding["fixedPoint"], json!([0.525, -1.1666666666666667]));
        assert_eq!(binding["mode"], json!("orbit"));
    }

    #[test]
    fn container_text_updates_or_creates_its_label() {
        let (next, _) = apply(&scene(), json!({"ops": [
            {"op": "update", "id": "r", "set": {"text": "renamed"}},
        ]})).unwrap();
        let Element::Text(label) = get(&next, "t") else { panic!("text") };
        assert_eq!(label.text, "renamed");
        assert_eq!(label.base.width, 7.0 * 20.0 * 0.6);

        let (next, report) = apply(&scene(), json!({"ops": [
            {"op": "add", "type": "ellipse", "id": "e", "x": 0, "y": 200, "width": 120, "height": 60},
            {"op": "update", "id": "e", "set": {"text": "later"}},
        ]})).unwrap();
        let e = report.created["e"].clone();
        let bound = get(&next, &e).bound_elements();
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].1, "text");
    }

    #[test]
    fn raw_elements_only_move_and_labels_do_not_move_alone() {
        let (next, _) = apply(&scene(), json!({"ops": [
            {"op": "update", "id": "img", "set": {"x": 310, "y": 5}},
        ]})).unwrap();
        assert_eq!(get(&next, "img").placement().unwrap().x, 310.0);
        let errors = apply(&scene(), json!({"ops": [
            {"op": "update", "id": "t", "set": {"x": 3}},
        ]})).unwrap_err();
        assert_eq!(errors[0].field.as_deref(), Some("set.x"));
    }

    #[test]
    fn delete_takes_the_label_and_unbinds_the_arrow() {
        let (next, report) = apply(&scene(), json!({"ops": [
            {"op": "delete", "ids": ["r"]},
        ]})).unwrap();
        assert_eq!(report.deleted, vec!["r"]);
        assert!(get(&next, "r").is_deleted() && get(&next, "t").is_deleted());
        assert_eq!(get(&next, "a").binding_target(LinearEnd::Start), None);
    }

    #[test]
    fn adds_run_before_updates_and_every_change_bumps_once() {
        let file = scene();
        let (next, report) = apply(&file, json!({"ops": [
            {"op": "update", "id": "b", "set": {"strokeColor": "#e03131"}},
            {"op": "add", "type": "rectangle", "id": "b", "x": 0, "y": 300, "width": 50, "height": 50},
        ]})).unwrap();
        let id = &report.created["b"];
        assert_eq!(report.added, vec![id.clone()]);
        assert_eq!(get(&next, id).to_value()["strokeColor"], json!("#e03131"));
        // Untouched elements keep their exact JSON, version included.
        assert_eq!(get(&next, "img"), get(&file, "img"));
    }

    #[test]
    fn setting_a_field_to_its_current_value_does_not_bump() {
        let file = scene();
        let (next, _) = apply(&file, json!({"ops": [
            {"op": "update", "id": "r", "set": {"x": 0}},
        ]})).unwrap();
        assert_eq!(get(&next, "r"), get(&file, "r"));
    }
```

`TestEnv` 用 `edit.rs` 測試裡同樣的寫法（每次 `fill_random` 遞增 37，時間固定 42）。第二個測試的期望值由 `fixed_point_for` 的公式手算：新矩形 `x = 0, y = 100, width = 200, height = 60`，端點 `(105, 30)`：`(105 - 0) / 200 = 0.525`、`(30 - 100) / 60 = -1.1666…`；標籤 `x = 5 + (190 / 2 - 48 / 2) = 76`、`y = 105 + (50 / 2 - 25 / 2) = 117.5`。實作若得到不同數字，先確認手算與 JS 公式一致再決定改哪邊。

Run: `cargo test -p scene batch::tests`
Expected: 編譯失敗。

- [ ] **Step 2：實作**

`apply_batch`：驗證頂層（物件、`ops` 是非空陣列、每個 op 是物件且 `op` 為 `add`／`update`／`delete`）；`let mut next = file.clone();`；收集 add → `add_elements(&mut next, …)`，錯誤先存起來但繼續檢查 update／delete（代號在 add 失敗時無法解析，那些 op 的錯誤照樣回報，訊息可能是 `no element`）；update／delete 照順序在 `next` 上執行，錯誤累積；最後有錯就 `Err(errors)`（依 `op` 排序），沒錯回 `Ok((next, report))`。id 解析：先查 `report.created`，再查 `next` 裡未刪除的元件。

每個 update 先整體驗證 `set`（所有 key 都合法才動元件），再套用，避免半套用。

Run: `cargo test -p scene batch`
Expected: PASS。

- [ ] **Step 3：全部檢查並 commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/scene/src/batch
git commit -m "Apply napkin batches: add, update and delete, all or nothing"
```

---

### Task 4：`Editor::apply_batch` 與歷史紀錄

**Files:**
- Modify: `crates/scene/src/editor/mod.rs`、`crates/scene/src/history.rs`
- Create: `crates/scene/tests/editor_batch.rs`

**Interfaces:**
- Consumes: Task 3 的 `batch::{apply_batch, BatchReport, OpError}`。
- Produces:

```rust
impl<E: Env> Editor<E> {
    /// Applies one batch as one history step (AI spec §4.1), keeping the selection (minus
    /// anything the batch deleted). Refuses while a gesture is in progress: the caller waits
    /// for `is_idle`. Bumps `revision` on success.
    pub fn apply_batch(&mut self, batch: &Value, measure: &mut dyn TextMeasure)
        -> Result<BatchReport, Vec<OpError>>;
}
```

- [ ] **Step 1：失敗的測試**

`crates/scene/tests/editor_batch.rs`：

```rust
mod support;

use scene::editor::{Command, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;

use support::{at, editor};

#[test]
fn a_batch_is_one_undo_step_and_keeps_the_selection() {
    let mut editor = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 50.0, 50.0])]);
    editor.command(Command::SelectAll);
    let before = editor.file().as_ref().clone();
    let revision = editor.revision();
    let report = editor
        .apply_batch(
            &json!({"ops": [
                {"op": "add", "type": "rectangle", "id": "a", "x": 100, "y": 0, "width": 50, "height": 50},
                {"op": "add", "type": "arrow", "x": 55, "y": 25, "points": [[0, 0], [40, 0]],
                 "start": {"id": "r"}, "end": {"id": "a"}},
            ]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    assert_eq!(report.added.len(), 2);
    assert_eq!(editor.revision(), revision + 1);
    assert!(editor.selection().contains("r"));
    assert!(editor.command(Command::Undo));
    assert_eq!(editor.file().as_ref(), &before);
    assert!(editor.command(Command::Redo));
    assert_eq!(editor.file().elements.len(), 3);
}

#[test]
fn a_failed_batch_changes_nothing_and_records_nothing() {
    let mut editor = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 50.0, 50.0])]);
    let before = editor.file().clone();
    let errors = editor
        .apply_batch(&json!({"ops": [{"op": "delete", "ids": ["missing"]}]}), &mut CharWidthMeasure)
        .unwrap_err();
    assert_eq!(errors[0].op, Some(0));
    assert_eq!(editor.file(), &before);
    assert!(!editor.command(Command::Undo));
}

#[test]
fn deleting_a_selected_element_drops_it_from_the_selection() {
    let mut editor = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 50.0, 50.0])]);
    editor.command(Command::SelectAll);
    editor
        .apply_batch(&json!({"ops": [{"op": "delete", "ids": ["r"]}]}), &mut CharWidthMeasure)
        .unwrap();
    assert!(editor.selection().is_empty());
}

#[test]
fn refuses_while_a_gesture_is_in_progress() {
    let mut editor = editor(vec![]);
    editor.set_tool(Tool::Rectangle);
    editor.pointer_down(at(0.0, 0.0));
    let errors = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "text", "x": 0, "y": 0, "text": "hi"}]}),
            &mut CharWidthMeasure,
        )
        .unwrap_err();
    assert!(errors[0].message.contains("gesture"), "{errors:?}");
}
```

Run: `cargo test -p scene --test editor_batch`
Expected: 編譯失敗。

- [ ] **Step 2：實作 `apply_batch`**

```rust
    pub fn apply_batch(
        &mut self,
        batch: &Value,
        measure: &mut dyn TextMeasure,
    ) -> Result<BatchReport, Vec<OpError>> {
        if !self.is_idle() {
            return Err(vec![OpError {
                op: None,
                field: None,
                message: "napkin is in the middle of a drawing gesture; try again".into(),
            }]);
        }
        let (next, report) =
            batch::apply_batch(&self.file, batch, &self.style, measure, &mut self.env)?;
        let before = std::mem::replace(&mut self.file, Arc::new(next));
        let selection = self.selection.clone();
        self.finish_edit(&before, &selection);
        Ok(report)
    }
```

`apply_batch` 對同一個場景跑成功時一定有改變（add 或 update 真的有變），但 update 把值設成原值的批次可能沒有改變：`finish_edit` 這時不記錄、不增加 `revision`，這是對的。

- [ ] **Step 3：`History::record` 不再深拷貝沒變的元件**

`history.rs` 的 `diff` 改成先比較參考、只在不相等時複製：

```rust
fn diff(before: &SceneFile, after: &SceneFile) -> Vec<Change> {
    let len = before.elements.len().max(after.elements.len());
    (0..len)
        .filter_map(|position| {
            let before_element = before.elements.get(position);
            let after_element = after.elements.get(position);
            (before_element != after_element).then(|| Change {
                position,
                before: before_element.cloned(),
                after: after_element.cloned(),
            })
        })
        .collect()
}
```

既有的 history 測試與 `undo_property` 照樣通過即可，行為不變。

Run: `cargo test -p scene`
Expected: PASS。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene/src/editor/mod.rs crates/scene/src/history.rs crates/scene/tests/editor_batch.rs
git commit -m "Apply a batch as one undo step; stop cloning unchanged elements in history diffs"
```

---

### Task 5：控制協定、摘要格式與請求處理

**Files:**
- Create: `crates/app/src/control/mod.rs`、`crates/app/src/control/summary.rs`、`crates/app/src/control/handler.rs`
- Modify: `crates/app/src/lib.rs`、`crates/app/Cargo.toml`、`Cargo.toml`（workspace 依賴加 `png = "0.18"`，app 加 `serde.workspace = true`；`png` 在 Task 6 才用到，這一步只加 `serde`）

**Interfaces:**
- Consumes: Task 4 的 `Editor::apply_batch`；Task 1 的 `TextMeasure`；`app::camera::Camera`。
- Produces:

```rust
// app::control
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "lowercase")]
pub enum Request {
    Status,
    Scene { #[serde(default)] full: bool },
    Selection { #[serde(default)] full: bool },
    View,
    Apply { batch: serde_json::Value },
    Render { out: std::path::PathBuf, target: RenderTarget },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderTarget { All, Selection, View }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response { pub ok: bool, pub output: String }
impl Response { pub fn ok(output: impl Into<String>) -> Response; pub fn error(output: impl Into<String>) -> Response; }

// app::control::summary
pub fn format_number(value: f64) -> String;                    // 2 decimals, trailing zeros trimmed, "-0" -> "0"
pub fn element_lines(file: &SceneFile, positions: &[usize]) -> Vec<String>;
pub fn full_lines(file: &SceneFile, positions: &[usize]) -> Vec<String>;

// app::control::handler
pub struct Session<'a, E: Env> {
    pub editor: &'a mut Editor<E>,
    pub path: Option<&'a Path>,
    pub unsaved: bool,
    pub save_error: Option<&'a str>,
    /// Why the canvas is read-only (an unparseable reload, spec §8), if it is.
    pub readonly: Option<&'a str>,
    pub camera: Camera,
    /// The canvas size in logical points.
    pub canvas_size: [f64; 2],
    pub measure: &'a mut dyn TextMeasure,
}
/// Whether the request changes the scene, and so must wait for `Editor::is_idle`.
pub fn is_mutating(request: &Request) -> bool;
pub fn handle<E: Env>(session: &mut Session<'_, E>, request: &Request) -> Response;
```

回應內容（決定 5、6）：

- `status`：
  ```
  file /home/leon/Documents/napkin/scratch.excalidraw
  unsaved no
  save_error none
  elements 12
  readonly no
  ```
  沒有路徑時 `file (none)`；`readonly` 唯讀時是 `readonly yes: <原因>`；`elements` 是未刪除元件數。
- `scene`／`selection`：決定 6 的格式。`selection` 用 `editor.selection().positions(file)`；`--full` 時再加上被選容器與箭頭的綁定文字。
- `view`：`<x> <y> <w> <h> zoom=<z>`，`x y w h` 是 `camera.visible_rect(canvas_size)` 的範圍。
- `apply`：唯讀時 `Response::error` 並帶 `{"errors":[{"message":"the canvas is read-only: <原因>"}]}`；成功時 `Response::ok` 帶一行 JSON `{"revision":N,"created":{...},"added":[...],"updated":[...],"deleted":[...],"warnings":[...]}`；失敗時 `Response::error` 帶 `{"errors":[...]}`（`OpError` 的序列化）。
- `render`：這個任務先回 `Response::error("render is not available")`，Task 6 實作。

- [ ] **Step 1：`summary` 的失敗測試**

```rust
#[cfg(test)]
mod tests {
    use scene::sample;
    use serde_json::json;

    use super::*;

    #[test]
    fn numbers_are_short() {
        assert_eq!(format_number(100.0), "100");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(1.0 / 3.0), "0.33");
        assert_eq!(format_number(-0.001), "0");
    }

    #[test]
    fn one_line_per_element_with_labels_folded_in() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 160.0, 70.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}],
                       "backgroundColor": "#a5d8ff", "groupIds": ["g1"]}),
            ),
            sample::text("t", [50.0, 22.5, 60.0, 25.0], "Parser", Some("r")),
            sample::with(
                sample::linear("arrow", "a", [165.0, 35.0], &[[0.0, 0.0], [100.0, 0.0]]),
                json!({"startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [1.03, 0.5001]},
                       "strokeColor": "#e03131"}),
            ),
            sample::text("free", [0.0, 100.0, 80.0, 25.0], "note\nline", None),
            sample::with(sample::generic("ellipse", "gone", [0.0, 0.0, 1.0, 1.0]), json!({"isDeleted": true})),
        ]);
        let positions: Vec<usize> = (0..file.elements.len()).collect();
        assert_eq!(
            element_lines(&file, &positions),
            vec![
                r##"r rectangle 0 0 160 70 label="Parser" bg=#a5d8ff groups=g1"##,
                r##"a arrow 165 35 100 0 stroke=#e03131 start=r points=[[0,0],[100,0]]"##,
                r##"free text 0 100 80 25 text="note\nline""##,
            ]
        );
    }
}
```

欄位順序固定為：`label`、`text`、`stroke`、`bg`、`start`、`end`、`points`、`groups`、`angle`、`locked`。`Raw` 元件沒有 `placement` 時 `x y w h` 印成 `? ? ? ?`。

- [ ] **Step 2：`handler` 的失敗測試**

`handler.rs` 的 `tests` 用 `scene::sample::CharWidthMeasure` 與一個固定的 `Env`（寫在測試模組裡）建 `Editor`：

```rust
    fn session<'a>(editor: &'a mut Editor<FixedEnv>, measure: &'a mut CharWidthMeasure) -> Session<'a, FixedEnv> {
        Session {
            editor,
            path: Some(Path::new("/tmp/napkin-test.excalidraw")),
            unsaved: false,
            save_error: None,
            readonly: None,
            camera: Camera { scroll_x: 0.0, scroll_y: 0.0, zoom: 1.0 },
            canvas_size: [800.0, 600.0],
            measure,
        }
    }

    #[test]
    fn status_scene_and_view() {
        let mut editor = Editor::new(sample::file(vec![sample::generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0])]), FixedEnv);
        let mut measure = CharWidthMeasure;
        let mut s = session(&mut editor, &mut measure);
        assert_eq!(
            handle(&mut s, &Request::Status),
            Response::ok("file /tmp/napkin-test.excalidraw\nunsaved no\nsave_error none\nelements 1\nreadonly no")
        );
        assert_eq!(
            handle(&mut s, &Request::Scene { full: false }),
            Response::ok("file /tmp/napkin-test.excalidraw\nr rectangle 0 0 10 10")
        );
        assert_eq!(handle(&mut s, &Request::View), Response::ok("0 0 800 600 zoom=1"));
    }

    #[test]
    fn apply_reports_ids_and_rejects_on_a_readonly_canvas() {
        let mut editor = Editor::new(sample::file(vec![]), FixedEnv);
        let mut measure = CharWidthMeasure;
        let mut s = session(&mut editor, &mut measure);
        let batch = json!({"ops": [{"op": "add", "type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 10, "height": 10}]});
        let response = handle(&mut s, &Request::Apply { batch: batch.clone() });
        assert!(response.ok, "{response:?}");
        let body: Value = serde_json::from_str(&response.output).unwrap();
        assert_eq!(body["revision"], json!(1));
        assert!(body["created"]["a"].is_string());

        s.readonly = Some("invalid JSON");
        let response = handle(&mut s, &Request::Apply { batch });
        assert!(!response.ok);
        assert!(response.output.contains("read-only: invalid JSON"), "{}", response.output);
    }

    #[test]
    fn requests_round_trip_as_json() {
        let request = Request::Render { out: "/tmp/x.png".into(), target: RenderTarget::Selection };
        let line = serde_json::to_string(&request).unwrap();
        assert_eq!(line, r#"{"command":"render","out":"/tmp/x.png","target":"selection"}"#);
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), request);
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"command":"scene"}"#).unwrap(),
            Request::Scene { full: false }
        );
        assert!(is_mutating(&Request::Apply { batch: json!({}) }));
        assert!(!is_mutating(&Request::Scene { full: true }));
    }
```

Run: `cargo test -p app control`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

`summary.rs`：`format_number` 用 `format!("{:.2}", value)` 再去掉尾端的 `0` 與 `.`，結果是 `-0` 時改成 `0`。`element_lines`：跳過已刪除元件、以及 `container_id()` 指向一個未刪除元件的文字；`label` 從容器 `bound_elements()` 裡 `type == "text"` 的那個活元件取 `text`（`serde_json::to_string` 產生帶引號的 JSON 字串）。`full_lines`：每個元件 `serde_json::to_string(&element.to_value())`。

`handler.rs`：照上面的回應內容實作。`apply` 的 `revision` 取 `editor.revision()`。

`control/mod.rs` 放 `Request`、`RenderTarget`、`Response`，並宣告 `pub mod handler; pub mod summary;`；`lib.rs` 加 `pub mod control;`。

Run: `cargo test -p app control`
Expected: PASS。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "Add the napkin control protocol, scene summaries and the request handler"
```

---

### Task 6：`render`：離螢幕渲染與 PNG

**Files:**
- Create: `crates/app/src/render/offscreen.rs`、`crates/app/src/control/render.rs`、`crates/app/tests/gpu_render.rs`
- Modify: `crates/app/src/render/mod.rs`、`crates/app/src/control/mod.rs`、`crates/app/src/control/handler.rs`、`crates/app/tests/support/mod.rs`、`crates/app/Cargo.toml`（`png.workspace = true`）、`Cargo.toml`（`png = "0.18"`）

**Interfaces:**
- Consumes: `CanvasRenderer::{new, prepare, paint}`、`CanvasFrame`、`gpu::STENCIL_FORMAT`、`render::color::render_color`、`scene::geometry::GeometryCache`、`Camera`。
- Produces:

```rust
// app::render::offscreen
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Draws `frame` into a fresh 4x MSAA texture cleared to the file's view background (dark mode
/// applied when `frame.dark`) and reads it back as tightly packed RGBA8 rows.
/// `renderer` must have been created with [`FORMAT`].
pub fn render_rgba(device: &wgpu::Device, queue: &wgpu::Queue,
                   renderer: &mut CanvasRenderer, frame: &CanvasFrame) -> Vec<u8>;
/// A [`Rasterize`] over a live device; creates its renderer on first use.
pub struct GpuRasterizer<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a mut Option<CanvasRenderer>,
}

// app::control::render
pub trait Rasterize {
    /// RGBA8 rows of `size_px`, `scene` drawn with `camera` at pixels-per-point 1.
    fn rasterize(&mut self, scene: Arc<SceneFile>, camera: Camera, size_px: [u32; 2], dark: bool)
        -> Result<Vec<u8>, String>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct Plan { pub scene: SceneFile, pub camera: Camera, pub size_px: [u32; 2] }
pub const PADDING_PX: f64 = 16.0;
pub const LONG_EDGE_PX: f64 = 1536.0;
pub const MAX_SIDE_PX: u32 = 4096;
pub fn plan(file: &SceneFile, selection: &Selection, target: RenderTarget,
            camera: Camera, canvas_size: [f64; 2]) -> Result<Plan, String>;
pub fn encode_png(rgba: &[u8], size_px: [u32; 2]) -> Vec<u8>;

// app::control::handler::Session gains
    pub dark: bool,
    pub rasterizer: &'a mut dyn Rasterize,
```

- [ ] **Step 1：搬離螢幕渲染**

把 `crates/app/tests/support/mod.rs` 裡 `render_sequence_with_ppp` 從建立材質到讀回 RGBA 的那段搬到 `render/offscreen.rs` 的 `render_rgba`（`FORMAT` 也搬過去），`render/mod.rs` 加 `pub mod offscreen;`。`support` 改成：建 renderer（`CanvasRenderer::new(&device, &queue, offscreen::FORMAT)`），對每個 `(file, generation)` 呼叫 `prepare` 並 submit，最後一個 frame 交給 `render_rgba`（它內部再 `prepare` 一次也可以，只要結果一樣；較簡單的做法是 `render_rgba` 自己呼叫 `prepare`，`support` 只對前面幾個 frame 呼叫 `prepare`）。`support::FORMAT` 改成 `pub use app::render::offscreen::FORMAT;`。

Run: `cargo test -p app --test gpu_shapes --test gpu_text`
Expected: PASS，行為不變。

- [ ] **Step 2：`plan` 的失敗測試**

`control/render.rs` 的 `tests`：

```rust
    #[test]
    fn all_fits_the_content_with_padding() {
        let file = sample::file(vec![
            sample::generic("rectangle", "a", [0.0, 0.0, 100.0, 50.0]),
            sample::generic("rectangle", "b", [200.0, 100.0, 100.0, 50.0]),
        ]);
        let plan = plan(&file, &Selection::new(), RenderTarget::All, Camera::default(), [800.0, 600.0]).unwrap();
        // Bounds include the stroke: element_bounds of a rectangle is its box.
        assert_eq!(plan.camera.zoom, 2.0);
        assert_eq!(plan.size_px, [632, 332]);
        assert_eq!(plan.camera.scene_to_view([0.0, 0.0]), [16.0, 16.0]);
    }

    #[test]
    fn selection_renders_only_selected_elements_and_their_labels() {
        let file = sample::file(vec![
            sample::with(sample::generic("rectangle", "a", [0.0, 0.0, 100.0, 50.0]),
                         json!({"boundElements": [{"id": "t", "type": "text"}]})),
            sample::text("t", [20.0, 12.5, 60.0, 25.0], "hi", Some("a")),
            sample::generic("rectangle", "b", [5000.0, 0.0, 10.0, 10.0]),
        ]);
        let plan = plan(&file, &Selection::from_ids(["a"]), RenderTarget::Selection, Camera::default(), [800.0, 600.0]).unwrap();
        let ids: Vec<_> = plan.scene.elements.iter().filter_map(|e| e.id()).collect();
        assert_eq!(ids, vec!["a", "t"]);
        assert!(plan.size_px[0] < 300);
        assert!(plan(&file, &Selection::new(), RenderTarget::Selection, Camera::default(), [800.0, 600.0]).is_err());
    }

    #[test]
    fn large_scenes_shrink_to_the_long_edge_and_view_uses_the_camera() {
        let file = sample::file(vec![sample::generic("rectangle", "a", [0.0, 0.0, 10000.0, 100.0])]);
        let all = plan(&file, &Selection::new(), RenderTarget::All, Camera::default(), [800.0, 600.0]).unwrap();
        // 10000 * (1536 / 10000) may land a hair above 1536 and round up.
        assert!((1536 + 32..=1537 + 32).contains(&all.size_px[0]), "{:?}", all.size_px);
        let camera = Camera { scroll_x: 3.0, scroll_y: 4.0, zoom: 1.5 };
        let view = plan(&file, &Selection::new(), RenderTarget::View, camera, [800.4, 600.0]).unwrap();
        assert_eq!((view.camera, view.size_px), (camera, [800, 600]));
        assert!(plan(&sample::file(vec![]), &Selection::new(), RenderTarget::All, camera, [800.0, 600.0]).is_err());
    }

    #[test]
    fn png_round_trips() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let bytes = encode_png(&rgba, [2, 1]);
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(&buf[..8], &rgba[..]);
    }
```

計算方式（`all`／`selection`）：`bounds = GeometryCache::default().common_bounds(元素)`；`cw = x2 - x1`、`ch = y2 - y1`；`scale = min(2, LONG_EDGE_PX / max(cw, ch, 1))`；`size_px = [ceil(cw * scale) + 2 * PADDING_PX, ceil(ch * scale) + 2 * PADDING_PX]`，每邊夾到 `1..=MAX_SIDE_PX`；`camera = Camera { zoom: scale, scroll_x: PADDING_PX / scale - x1, scroll_y: PADDING_PX / scale - y1 }`。`view`：相機照用，`size_px` 是 `canvas_size` 四捨五入、夾到 `1..=MAX_SIDE_PX`。`png` 0.18 的 API 若與測試裡的寫法不同，照 crate 的實際 API 調整測試。

Run: `cargo test -p app control::render`
Expected: 編譯失敗。

- [ ] **Step 3：實作 `plan`、`encode_png`、`GpuRasterizer` 與 handler 的 `render`**

`plan` 的 `scene`：`all` 是 `file` 的複本；`selection` 是只留選取元件、以及 `container_id` 指向被選元件的文字的複本（保留原順序與 `appState`）；`view` 是 `file` 的複本。

`GpuRasterizer::rasterize`：`renderer.get_or_insert_with(|| CanvasRenderer::new(device, queue, offscreen::FORMAT))`，組 `CanvasFrame { file: scene, camera, size_px, pixels_per_point: 1.0, dark, generation: 0 }`，呼叫 `render_rgba`。

handler 的 `render`：`plan` → `rasterizer.rasterize` → `encode_png` → `std::fs::write(out, bytes)`，成功回 `Response::ok(format!("{} {}x{}", out.display(), w, h))`，任何一步失敗回 `Response::error(訊息)`。`Session` 加 `dark` 與 `rasterizer` 欄位；Task 5 的測試補一個記錄呼叫的假 `Rasterize`（回傳全白的 RGBA），並加一個測試：`render` 寫出的檔案能用 `png::Decoder` 解開、寬高與 `plan` 一致。

Run: `cargo test -p app control`
Expected: PASS。

- [ ] **Step 4：GPU 測試**

`crates/app/tests/gpu_render.rs`：

```rust
mod support;

use app::control::handler::{Session, handle};
use app::control::{Request, RenderTarget};
use app::camera::Camera;
use app::render::offscreen::GpuRasterizer;
use scene::editor::Editor;
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;

#[test]
fn render_writes_a_png_of_the_scene() {
    let (_gpu, device, queue) = support::gpu();
    let mut renderer = None;
    let mut rasterizer = GpuRasterizer { device: &device, queue: &queue, renderer: &mut renderer };
    let fill = sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
        json!({"roughness": 0, "strokeColor": "#1971c2", "backgroundColor": "#1971c2", "fillStyle": "solid"}),
    );
    let mut editor = Editor::new(sample::file(vec![fill]), scene::env::SystemEnv);
    let mut measure = CharWidthMeasure;
    let out = std::env::temp_dir().join(format!("napkin-render-{}.png", std::process::id()));
    let mut session = Session {
        editor: &mut editor,
        path: None,
        unsaved: false,
        save_error: None,
        readonly: None,
        camera: Camera::default(),
        canvas_size: [800.0, 600.0],
        measure: &mut measure,
        dark: false,
        rasterizer: &mut rasterizer,
    };
    let response = handle(&mut session, &Request::Render { out: out.clone(), target: RenderTarget::All });
    assert!(response.ok, "{}", response.output);

    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (232, 132));
    let center = ((66 * info.width + 116) * 4) as usize;
    assert_eq!(&buf[center..center + 3], &[0x19, 0x71, 0xc2]);
    let corner = &buf[0..3];
    assert_eq!(corner, &[255, 255, 255], "view background");
    std::fs::remove_file(out).ok();
}
```

`232 = 100 × 2 + 32`、`132 = 50 × 2 + 32`（矩形外框就是它的方框，不含筆畫寬度；若 `element_bounds` 對矩形含筆畫，照實際值修改期望的尺寸與中心點）。

Run: `cargo test -p app --test gpu_render`
Expected: PASS。

- [ ] **Step 5：全部檢查並 commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "Render the canvas, the selection or the view to PNG"
```

---

### Task 7：socket 伺服器、子指令用戶端與整合測試

**Files:**
- Create: `crates/app/src/control/server.rs`、`crates/app/src/control/client.rs`、`crates/app/tests/control_socket.rs`
- Modify: `crates/app/src/control/mod.rs`、`crates/app/src/cli.rs`、`crates/app/src/main.rs`

**Interfaces:**
- Consumes: Task 5／6 的 `Request`、`Response`、`handler::handle`。
- Produces:

```rust
// app::control::server
/// `$NAPKIN_SOCKET` when set (tests), else `$XDG_RUNTIME_DIR/napkin.sock`; `None` when neither.
pub fn socket_path() -> Option<PathBuf>;
/// One request waiting for the UI thread's answer.
pub struct Incoming { pub request: Request, reply: mpsc::Sender<Response> }
impl Incoming { pub fn reply(self, response: Response); }
#[derive(Debug)]
pub enum BindError { AlreadyRunning, Io(std::io::Error) }
pub struct Server { .. }
impl Server {
    /// Binds `path` (0600), replacing a stale socket file nobody listens on. `wake` runs after
    /// every request that reached the queue (the app passes `ctx.request_repaint`).
    pub fn bind(path: &Path, wake: impl Fn() + Send + 'static) -> Result<Server, BindError>;
    pub fn try_recv(&self) -> Option<Incoming>;
}
impl Drop for Server { /* stops the thread, removes the socket file */ }

// app::control::client
#[derive(Debug)]
pub enum ClientError { NotRunning, Failed(String), Io(std::io::Error) }
/// Sends one request and waits up to 120 s. `Ok(output)` when napkin answered `ok`.
pub fn send(path: &Path, request: &Request) -> Result<String, ClientError>;
pub const NOT_RUNNING: &str = "napkin is not running; open it with SUPER+N";

// app::cli
#[derive(Debug, PartialEq)]
pub enum Command {
    Gui { file: Option<PathBuf>, bench: bool },
    Control(ControlCommand),
}
#[derive(Debug, PartialEq)]
pub enum ControlCommand {
    Status, Scene { full: bool }, Selection { full: bool }, View, Apply,
    Render { out: PathBuf, target: RenderTarget },
}
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String>;
```

伺服器行為：

- `bind`：`path` 存在時先 `UnixStream::connect`；連得上回 `AlreadyRunning`；連不上（`ConnectionRefused` 等）就刪檔再 `UnixListener::bind`，之後 `set_permissions(0o600)`。
- accept 迴圈在一個具名執行緒 `napkin-control`。每個連線設 5 秒讀取逾時，讀一行（上限 16 MiB，超過回錯誤），解析成 `Request`；解析失敗直接回 `{"ok":false,"output":"bad request: ..."}`。成功就把 `Incoming` 送進 channel、呼叫 `wake`，等回覆最多 120 秒（逾時回 `napkin did not answer in time`），寫一行 JSON 回應後關閉。一次處理一個連線。
- `Drop`：設停止旗標、自己連一次 socket 讓 `accept` 返回、join 執行緒、刪 socket 檔。

用戶端行為：連線失敗且錯誤是 `NotFound` 或 `ConnectionRefused` 時回 `NotRunning`；寫一行請求、讀一行回應（讀取逾時 120 秒）；`ok` 為 false 回 `Failed(output)`。

子指令語法（`USAGE` 也改成列出這些）：

```
napkin [FILE.excalidraw] [--bench]
napkin status
napkin scene [--full]
napkin selection [--full]
napkin view
napkin apply                 (reads {"ops": [...]} from stdin)
napkin render --out FILE.png [--selection | --view]
```

第一個參數是這些子指令名稱之一時就是子指令（要開名叫 `status` 的檔案寫 `./status`）。`render` 的 `--out` 必填，相對路徑用目前目錄轉成絕對路徑；`--selection` 與 `--view` 互斥。

`main.rs`：`Command::Control` 時不開視窗：`apply` 從 stdin 讀全部、解析成 JSON（失敗印 `napkin: stdin is not JSON: ...`、結束碼 2）；`socket_path()` 為 `None` 時當成沒在執行；成功把輸出印到 stdout（沒有換行結尾就補一個），`Failed` 印到 stderr、結束碼 1，`NotRunning` 印 `NOT_RUNNING`、結束碼 1。

- [ ] **Step 1：`cli` 的測試改寫（先失敗）**

既有兩個測試改成新的 `Command` 型別，並加：

```rust
    #[test]
    fn parses_control_subcommands() {
        assert_eq!(parse(args(&["status"])), Ok(Command::Control(ControlCommand::Status)));
        assert_eq!(parse(args(&["scene", "--full"])), Ok(Command::Control(ControlCommand::Scene { full: true })));
        assert_eq!(parse(args(&["apply"])), Ok(Command::Control(ControlCommand::Apply)));
        assert_eq!(
            parse(args(&["render", "--out", "/tmp/a.png", "--view"])),
            Ok(Command::Control(ControlCommand::Render { out: "/tmp/a.png".into(), target: RenderTarget::View }))
        );
        assert!(parse(args(&["render"])).is_err(), "--out is required");
        assert!(parse(args(&["render", "--out", "a.png", "--view", "--selection"])).is_err());
        assert!(parse(args(&["scene", "extra"])).is_err());
        assert_eq!(
            parse(args(&["./status"])),
            Ok(Command::Gui { file: Some("./status".into()), bench: false })
        );
    }
```

- [ ] **Step 2：整合測試（先失敗）**

`crates/app/tests/control_socket.rs`：

```rust
//! A windowless server (socket + Editor, no GUI) driven by the real `napkin` binary.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use app::camera::Camera;
use app::control::handler::{Session, handle};
use app::control::render::Rasterize;
use app::control::server::{BindError, Server};
use scene::editor::Editor;
use scene::sample::{self, CharWidthMeasure};

struct NoGpu;

impl Rasterize for NoGpu {
    fn rasterize(&mut self, _: std::sync::Arc<scene::SceneFile>, _: Camera, _: [u32; 2], _: bool)
        -> Result<Vec<u8>, String> {
        Err("no GPU in this test".into())
    }
}

fn socket(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("napkin-test-{}-{name}.sock", std::process::id()))
}

/// Serves requests until the returned sender is dropped... (a thread that owns the Editor,
/// polls `server.try_recv()` every 5 ms and answers with `handle`).
fn spawn_server(path: PathBuf) -> std::thread::JoinHandle<()> { .. }

fn napkin(path: &std::path::Path, args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_napkin"))
        .args(args)
        .env("NAPKIN_SOCKET", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn subcommands_talk_to_a_running_server() {
    let path = socket("talk");
    let server = spawn_server(path.clone());
    let out = napkin(&path, &["apply"], r#"{"ops": [{"op": "add", "type": "rectangle", "id": "a",
        "x": 0, "y": 0, "width": 40, "height": 20}]}"#);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let body: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = body["created"]["a"].as_str().unwrap().to_owned();

    let out = napkin(&path, &["scene"], "");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(&format!("{id} rectangle 0 0 40 20")), "{text}");

    let out = napkin(&path, &["apply"], r#"{"ops": [{"op": "delete", "ids": ["nope"]}]}"#);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nope"));

    let out = napkin(&path, &["render", "--out", "/tmp/unused.png"], "");
    assert_eq!(out.status.code(), Some(1));
    stop(server, &path);
}

#[test]
fn not_running_is_an_error_without_side_effects() {
    let path = socket("absent");
    let out = napkin(&path, &["status"], "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "napkin is not running; open it with SUPER+N");
}

#[test]
fn a_stale_socket_is_replaced_and_a_live_one_is_not() {
    let path = socket("stale");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap()); // leaves the file behind
    let first = Server::bind(&path, || {}).expect("stale file replaced");
    assert!(matches!(Server::bind(&path, || {}), Err(BindError::AlreadyRunning)));
    let mode = std::fs::metadata(&path).unwrap().permissions();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
    drop(first);
    assert!(!path.exists(), "the server removes its socket file");
}
```

`spawn_server` 與 `stop` 的實作留給實作者：伺服器執行緒持有 `Server` 與 `Editor::new(sample::file(vec![]), scene::env::SystemEnv)`，用一個 `Arc<AtomicBool>` 停止；每次 `try_recv` 拿到請求就建 `Session`（`CharWidthMeasure`、`NoGpu`、`canvas_size [800, 600]`）呼叫 `handle` 並 `reply`。

Run: `cargo test -p app --test control_socket && cargo test -p app cli`
Expected: 編譯失敗。

- [ ] **Step 3：實作 server、client、cli、main**

照上面的行為實作。`server.rs` 的模組 doc comment 寫明 `NAPKIN_SOCKET` 是測試用的覆寫。

Run: `cargo test -p app`
Expected: PASS。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Serve control requests over a Unix socket and add napkin subcommands"
```

---

### Task 8：把控制 socket 接進 `NapkinApp`

**Files:**
- Modify: `crates/app/src/napkin_app.rs`、`crates/app/src/main.rs`（傳 `bench` 以外不用改；`NapkinApp::new` 自己開 socket）

**Interfaces:**
- Consumes: Task 5 到 7 的 `Server`、`Incoming`、`handler::{Session, handle, is_mutating}`、`GpuRasterizer`、`FontMeasure`。
- Produces: 沒有新的公開介面。`NapkinApp` 新增欄位：

```rust
    /// `None` in `--bench`, without `$XDG_RUNTIME_DIR`, or when another napkin owns the socket.
    control: Option<Server>,
    /// Requests in arrival order; a mutating one at the front waits for `Editor::is_idle`.
    pending: VecDeque<Incoming>,
    /// Created on the first request that measures text.
    measure: Option<FontMeasure>,
    /// The renderer `render` requests draw with (its own format, `offscreen::FORMAT`).
    offscreen: Option<CanvasRenderer>,
    /// The canvas size in points, from the latest laid-out frame.
    canvas_size: [f64; 2],
```

行為：

- `new`：不是 `bench` 時 `server::socket_path()` → `Server::bind(path, 喚醒 egui)`。`AlreadyRunning` 時設 notice `another napkin owns the control socket; napkin commands go to it`；`Io` 錯誤 `eprintln!` 並設 notice `control socket unavailable: <錯誤>`。
- 每幀：`control.try_recv()` 全部移進 `pending`。
- 啟動就無法解析檔案（`load_error`，沒有 `Editor`）：每個請求直接回 `Response::error(format!("napkin could not open the canvas: {load_error}"))`。
- 有 `Editor` 且相機已經初始化時，在處理完使用者輸入、重新載入檢查之後、自動存檔判斷之前，從 `pending` 前端開始處理：前端是修改類請求、畫布不是唯讀、`editor.is_idle()` 為 false 時停下，並 `request_repaint_after(50 ms)`；否則取出、建 `Session` 呼叫 `handle`、`reply`。`Session` 的 `unsaved` 是 `autosave.has_unsaved_changes(editor.revision())`，`save_error` 是 `autosave.error()`，`readonly` 是 `self.unreadable`，`dark` 是 `self.theme.dark`，`rasterizer` 是用 `frame.wgpu_render_state()` 的 device／queue 與 `self.offscreen` 建的 `GpuRasterizer`（沒有 render state 時用一個回 `Err("no GPU")` 的 `Rasterize`）。
- 相機還沒初始化（第一個有尺寸的畫面之前）時不處理，請求留在佇列。
- `on_exit`：先對 `pending` 每個請求回 `napkin is closing`，再照舊存檔；`Server` 在 `NapkinApp` drop 時刪 socket 檔。

- [ ] **Step 1：實作**

`ui()` 裡找到 `if let Some(editor) = self.editor.as_mut()` 那段、在自動存檔判斷之前插入處理佇列的呼叫；把處理寫成 `NapkinApp` 的一個私有方法 `serve_requests(&mut self, frame: &eframe::Frame, ctx: &egui::Context, camera: Camera)`，避免 `ui()` 再變長。借用衝突時先把需要的值（路徑、unsaved、錯誤字串）複製成區域變數。

`canvas_size` 在 `allocate_exact_size` 之後更新。

- [ ] **Step 2：確認沒有壞掉**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: 全部通過。

- [ ] **Step 3：煙霧測試**

在隔離的環境開一次視窗，確認 socket 真的能用（不送任何按鍵）：

```bash
cargo build --release -p app
export HOME_T=$(mktemp -d)
HOME=$HOME_T XDG_RUNTIME_DIR=$HOME_T target/release/napkin "$HOME_T/t.excalidraw" &
sleep 3
NAPKIN_SOCKET=$HOME_T/napkin.sock target/release/napkin status
echo '{"ops":[{"op":"add","type":"rectangle","id":"a","x":0,"y":0,"width":160,"height":70,"label":{"text":"hello"},"backgroundColor":"#a5d8ff"}]}' \
  | NAPKIN_SOCKET=$HOME_T/napkin.sock target/release/napkin apply
NAPKIN_SOCKET=$HOME_T/napkin.sock target/release/napkin scene
NAPKIN_SOCKET=$HOME_T/napkin.sock target/release/napkin render --out $HOME_T/all.png
```

Expected: `status` 顯示 `$HOME_T/t.excalidraw`；`apply` 印出含 `created` 的 JSON；`scene` 有一行 `rectangle ... label="hello"`；`all.png` 用 Read 看得到藍底方塊與文字。然後用 `grim` 截 napkin 視窗確認方塊出現在畫布上，關掉視窗（`hl.dsp.focus` 用 `class:napkin`，再 `hl.dsp.window.close()`），等 1 秒確認 `$HOME_T/napkin.sock` 已刪除、`$HOME_T/t.excalidraw` 裡有那個矩形。注意：這台機器可能已經開著使用者的 napkin，`hl.dsp.focus({ window = "class:napkin" })` 可能聚焦到它；截圖與關閉前先用 `hyprctl clients -j` 找 `pid` 等於剛才背景行程的那個視窗，改用 `pid:<pid>` 聚焦。

- [ ] **Step 4：commit**

```bash
git add crates/app/src/napkin_app.rs crates/app/src/main.rs
git commit -m "Answer control requests from the running canvas"
```

---

### Task 9：完整路徑與 `Ctrl+K`

**Files:**
- Create: `crates/app/src/agent.rs`
- Modify: `crates/app/src/lib.rs`、`crates/app/src/storage.rs`、`crates/app/src/main.rs`、`crates/app/src/napkin_app.rs`

**Interfaces:**
- Produces:

```rust
// app::storage
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayPath {
    /// The folder with a trailing `/`, `~`-abbreviated under `home`.
    pub dir: String,
    pub name: String,
}
pub fn display_path(path: &Path, home: Option<&Path>) -> DisplayPath;

// app::agent
pub const APP_ID: &str = "org.napkin.agent";
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action { Focus, Launch }
/// `Focus` when `hyprctl clients -j` lists a window whose class is [`APP_ID`].
pub fn action(clients_json: &str) -> Action;
/// `xdg-terminal-exec` and its arguments for `home`.
pub fn launch_command(home: &Path) -> (String, Vec<String>);
/// Runs `hyprctl clients -j`, then focuses or launches; creates `~/Documents/napkin` first.
pub fn open(home: &Path) -> Result<(), String>;
```

`launch_command` 回傳 `("xdg-terminal-exec", ["--app-id=org.napkin.agent", "--dir=<home>/Documents/napkin", "--", "claude", "--continue", "--dangerously-skip-permissions", "--model", "sonnet"])`。`--continue` 在沒有對話紀錄的資料夾會直接開新對話（2026-09-26 用 `claude --continue -p hi` 在空資料夾確認過），不需要退路。`open`：`hyprctl` 執行失敗時當成沒有視窗；聚焦用 `hyprctl dispatch 'hl.dsp.focus({ window = "class:org.napkin.agent" })'`；啟動用 `Command::spawn`，開一個執行緒 `wait()` 回收子行程。

- [ ] **Step 1：失敗的測試**

`storage.rs`：

```rust
    #[test]
    fn display_path_abbreviates_home() {
        let home = Path::new("/home/leon");
        assert_eq!(
            display_path(Path::new("/home/leon/Documents/napkin/scratch.excalidraw"), Some(home)),
            DisplayPath { dir: "~/Documents/napkin/".into(), name: "scratch.excalidraw".into() }
        );
        assert_eq!(
            display_path(Path::new("/tmp/a.excalidraw"), Some(home)),
            DisplayPath { dir: "/tmp/".into(), name: "a.excalidraw".into() }
        );
        assert_eq!(
            display_path(Path::new("/home/leonard/x.excalidraw"), Some(home)).dir,
            "/home/leonard/",
            "only whole path components match"
        );
    }
```

`agent.rs`：

```rust
    #[test]
    fn focuses_an_existing_agent_window() {
        let clients = r#"[{"class": "foot", "pid": 1}, {"class": "org.napkin.agent", "pid": 2}]"#;
        assert_eq!(action(clients), Action::Focus);
        assert_eq!(action(r#"[{"class": "napkin"}]"#), Action::Launch);
        assert_eq!(action("not json"), Action::Launch);
    }

    #[test]
    fn launches_claude_in_the_napkin_folder() {
        let (program, args) = launch_command(Path::new("/home/leon"));
        assert_eq!(program, "xdg-terminal-exec");
        assert_eq!(args, [
            "--app-id=org.napkin.agent", "--dir=/home/leon/Documents/napkin", "--",
            "claude", "--continue", "--dangerously-skip-permissions", "--model", "sonnet",
        ]);
    }
```

Run: `cargo test -p app storage agent`
Expected: 編譯失敗。

- [ ] **Step 2：實作 `display_path` 與 `agent`**

`display_path`：`path.strip_prefix(home)` 成功就 `~/` 加剩下的資料夾部分；資料夾部分保證以 `/` 結尾；`name` 是 `file_name()`，沒有時整條路徑放進 `name`、`dir` 為空。

- [ ] **Step 3：接進 `NapkinApp` 與 `main.rs`**

- `main.rs`：`name_of` 改成回傳 `DisplayPath`（有路徑時 `display_path(path, home)`，沒有時 `dir` 空、`name` 是 `untitled`），視窗標題 `format!("{}{} - napkin", dir, name)`。`NapkinApp::new` 的 `name: String` 參數改成 `display: DisplayPath`。
- 右上角：`ui.horizontal` 裡 `dir` 用 `egui::RichText::new(dir).weak()`、`name` 用一般 `RichText`，兩個都用 `egui::Label::new(..).sense(egui::Sense::click())`；任一個被點到就 `ui.ctx().copy_text(絕對路徑)` 並設 notice `Copied <絕對路徑>`。沒有路徑時點了不做事。
- `Ctrl+K`：不在 `--bench`、有 `Editor`、egui 沒拿走鍵盤時，`ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K))` 為真就呼叫 `agent::open(home)`（`home` 從 `storage::Paths::from_env()` 或 `std::env::var_os("HOME")`），`Err` 設成 notice。

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: 全部通過。

- [ ] **Step 4：煙霧測試**

照 Task 8 Step 3 的方式在暫時的 HOME 開 napkin，`grim` 截圖確認右上角是 `~/t.excalidraw` 形式（暫時的 HOME 下是 `~/`）且資料夾較淡、`hyprctl clients -j` 裡該視窗的 `title` 是 `~/t.excalidraw - napkin`。關閉視窗。`Ctrl+K` 需要按鍵，交給使用者在最後的人工驗收測。

- [ ] **Step 5：commit**

```bash
git add crates/app
git commit -m "Show the canvas path in the corner and title; open Claude Code with Ctrl+K"
```

---

### Task 10：M4a 延後項目：Super 鍵與刪檔後的唯讀

**Files:**
- Modify: `crates/app/src/pinch.rs`、`crates/app/src/edit_input.rs`、`crates/app/src/napkin_app.rs`

**Interfaces:**
- Produces:

```rust
// app::pinch
/// Linux evdev `KEY_LEFTMETA` / `KEY_RIGHTMETA`.
pub const SUPER_KEYS: [u32; 2] = [125, 126];
/// Which Super keys are down, from `wl_keyboard` `enter`/`key`/`leave`.
#[derive(Debug, Default)]
pub struct SuperTracker { .. }
impl SuperTracker {
    pub fn enter(&mut self, pressed_keys: &[u32]);
    pub fn key(&mut self, key: u32, pressed: bool);
    pub fn leave(&mut self);
    pub fn held(&self) -> bool;
}
impl PinchListener {
    /// Whether a Super key is down right now; `false` when the keyboard was never bound.
    pub fn super_held(&self) -> bool;
}

// app::edit_input::FrameInput gains
    /// A Super key is down: Hyprland passes Super+letter combinations it does not bind to the
    /// focused window, and egui-winit drops the Super modifier on Linux, so the letter alone
    /// would reach the tool shortcuts.
    pub super_held: bool,
```

背景：egui-winit 0.36 在 Linux 丟掉 Super 修飾鍵（只在 macOS 設 `mac_cmd`），Hyprland 沒綁的 SUPER+字母（H、A、R、E、Y、Z…）會以單一字母送進 napkin，把工具切掉。`PinchListener` 已經在 eframe 的 Wayland 連線上開了自己的 event queue；在 seat 的 `capabilities` 有 keyboard 時多綁一個 `wl_keyboard`，只看 `enter`（`keys` 陣列是 u32 little-endian 的 evdev 碼）、`key`、`leave`，更新一個 `Arc<Mutex<SuperTracker>>`（或兩個 `AtomicBool`）。`keymap` 事件帶的 fd 由 wayland-client 的 `OwnedFd` 自動關閉，不用處理。

- [ ] **Step 1：失敗的測試**

`pinch.rs`：

```rust
    #[test]
    fn tracks_either_super_key() {
        let mut tracker = SuperTracker::default();
        assert!(!tracker.held());
        tracker.key(125, true);
        tracker.key(126, true);
        tracker.key(125, false);
        assert!(tracker.held(), "right Super still down");
        tracker.key(126, false);
        assert!(!tracker.held());
        tracker.enter(&[30, 126]);
        assert!(tracker.held());
        tracker.leave();
        assert!(!tracker.held());
        tracker.key(30, true);
        assert!(!tracker.held(), "other keys do not count");
    }
```

`edit_input.rs`（`frame` 輔助函數加 `super_held: false`）：

```rust
    #[test]
    fn keys_are_ignored_while_super_is_held() {
        let events = [egui::Event::Key {
            key: egui::Key::H,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }];
        let mut input = frame(&events);
        input.super_held = true;
        assert!(translate(&input, &mut PointerCapture::default()).is_empty());
        input.super_held = false;
        assert_eq!(translate(&input, &mut PointerCapture::default()), vec![EditorInput::Tool(Tool::Hand)]);
    }
```

（`egui::Event::Key` 的欄位照 egui 0.36 實際定義調整。）

Run: `cargo test -p app pinch edit_input`
Expected: 編譯失敗。

- [ ] **Step 2：實作 Super 鍵**

`translate` 的 `Key` 分支加 `if !input.super_held` 條件；`NapkinApp` 組 `FrameInput` 時 `super_held: self.pinch.as_ref().is_some_and(PinchListener::super_held)`；`Ctrl+K` 的判斷也在 Super 按著時跳過。模組 doc comment 補一句 `PinchListener` 也追蹤 Super 鍵。

- [ ] **Step 3：重新載入失敗後檔案被刪就一直唯讀**

目前 `should_reload` 對 `disk_mtime == None` 回 `false`，重新載入失敗（`unreadable` 有值）之後使用者把壞掉的檔案刪了，畫布停在唯讀直到重啟。修正：重新載入檢查執行時，`self.unreadable.is_some()` 而檔案已不存在，就清掉 `unreadable`，`known_mtime` 設成 `None`，繼續編輯最後一次成功載入的場景（下次存檔會重新建立檔案）。把判斷寫成純函數並加測試：

```rust
/// A read-only canvas (a failed reload) whose file has since been deleted becomes editable
/// again: nothing on disk is left to protect, and the next save recreates the file.
pub fn should_clear_unreadable(unreadable: bool, disk_mtime: Option<SystemTime>) -> bool {
    unreadable && disk_mtime.is_none()
}
```

```rust
    #[test]
    fn a_deleted_unreadable_file_makes_the_canvas_editable_again() {
        let t = SystemTime::UNIX_EPOCH;
        assert!(should_clear_unreadable(true, None));
        assert!(!should_clear_unreadable(true, Some(t)));
        assert!(!should_clear_unreadable(false, None));
    }
```

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: 全部通過。

- [ ] **Step 4：commit**

```bash
git add crates/app
git commit -m "Ignore letter keys while Super is held; recover from a deleted unreadable file"
```

---

### Task 11：skill、機器設定與人工驗收

**Files:**
- Create: `skills/napkin/SKILL.md`、`crates/app/tests/skill_examples.rs`
- Modify: `CLAUDE.md`
- 機器設定（不在 repo）：`~/.claude/skills/napkin`、`~/.local/bin/napkin`、`~/.config/hypr/windows.lua`

- [ ] **Step 1：`skill_examples.rs`（先失敗）**

```rust
//! Every ```json block in skills/napkin/SKILL.md is a batch that must apply cleanly to an
//! empty scene, so the skill cannot drift from what `napkin apply` accepts.

use scene::editor::Editor;
use scene::sample::{self, CharWidthMeasure};

#[test]
fn skill_batches_apply_to_an_empty_scene() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/napkin/SKILL.md");
    let text = std::fs::read_to_string(path).expect("SKILL.md");
    let blocks: Vec<&str> = text
        .split("```json\n")
        .skip(1)
        .map(|rest| rest.split("```").next().expect("closed block"))
        .collect();
    assert!(blocks.len() >= 3, "the skill shows at least three batches");
    for block in blocks {
        let batch: serde_json::Value = serde_json::from_str(block).unwrap_or_else(|e| panic!("{e}\n{block}"));
        let mut editor = Editor::new(sample::file(vec![]), scene::env::SystemEnv);
        if let Err(errors) = editor.apply_batch(&batch, &mut CharWidthMeasure) {
            panic!("{errors:?}\n{block}");
        }
    }
}
```

需要既有元件才能示範的例子（`update`、`delete`）在同一個 JSON 區塊裡先 `add` 再用代號操作，這樣每個區塊都能獨立套用。

Run: `cargo test -p app --test skill_examples`
Expected: FAIL（檔案不存在）。

- [ ] **Step 2：寫 `skills/napkin/SKILL.md`**

英文（repo 慣例）。frontmatter：

```markdown
---
name: napkin
description: Read, draw on and look at the napkin whiteboard the user has open. Use when the user asks to draw, sketch, diagram or visualize something in napkin, to explain or implement from a drawing on the napkin canvas, or mentions "napkin", "the canvas" or "the whiteboard".
---
```

內容依序（實際的指令輸出以 Task 5 到 7 的實作為準，寫之前先在 Task 8 的煙霧測試環境跑一次每個指令、照實際輸出寫例子）：

1. 一段話：napkin 是常駐的手繪白板，`napkin` 指令操作使用者正在看的那張畫布，每批修改立刻出現、使用者按一次 Ctrl+Z 撤銷一批。
2. **Before drawing**：先 `napkin status`（哪個檔、是否唯讀）與 `napkin view`（使用者看得到的範圍），新東西放進 view 範圍；`napkin scene` 看既有內容，避免重疊。不要動使用者畫的東西，除非被要求。
3. **Reading**：`scene` 的每行格式（`<id> <type> <x> <y> <w> <h>` 與各欄位的意義），`--full` 何時用；`selection` 是使用者選取的東西；要理解手繪草圖時 `napkin render --out /tmp/napkin-selection.png --selection`（沒有選取就 `render --out /tmp/napkin.png`）再用 Read 看圖。
4. **Drawing**：`napkin apply <<'EOF' ... EOF` 的格式；`add` 各類型與欄位；`update`、`delete`；代號；一批先執行所有 add 再依序 update／delete；錯誤整批不動、回應列出第幾筆哪個欄位；`warnings` 表示標籤放不下，要放大框或加 `\n`。至少三個 ```json 例子：(a) 三個有標籤的方塊與兩支綁定的箭頭（完整一步）、(b) 一段置中的標題文字與一條虛線、(c) 先 add 再 update 改色、再 delete 的組合。
5. **Layout conventions**：字寬約 `0.55 × fontSize`（拉丁字母）、約 `1 × fontSize`（中文），框的寬度至少字寬加 40、一行字的框高 60 到 80；間距 60 到 100；座標對齊 20 的倍數；主要流程由左到右或由上到下；箭頭從形狀邊緣外 6 到 10 px 開始、到另一個形狀邊緣前 6 到 10 px 結束，並用 `start`／`end` 綁定；配色用 Excalidraw 調色盤成對的淡底深框（藍 `#1971c2`／`#a5d8ff`、綠 `#2f9e44`／`#b2f2bb`、紅 `#e03131`／`#ffc9c9`、黃 `#f08c00`／`#ffec99`、紫 `#6741d9`／`#d0bfff`），`fillStyle` 用 `solid`；同一張圖顏色不超過三組；文字元件的 `x` 在 `textAlign: center` 時是中心。
6. **Pacing**：分批送，一批是一個有意義的步驟（先框、再箭頭、再標註），讓使用者看到圖長出來；一批不要超過約 15 個元件。
7. **Check your work**：畫完 `napkin render --out /tmp/napkin.png` 並用 Read 看，有重疊、文字溢出、箭頭沒接上就修，再回報。
8. **Errors**：`napkin is not running` 表示要請使用者按 SUPER+N；唯讀畫布不能改；`napkin is in the middle of a drawing gesture` 之類的忙碌訊息稍後重試。

Run: `cargo test -p app --test skill_examples`
Expected: PASS。

- [ ] **Step 3：`CLAUDE.md`**

在「改壞了也不會有錯誤訊息的地方」加一條：

```markdown
- **`skills/napkin/SKILL.md` 描述 `napkin` 子指令的輸出與 `apply` 格式**：改 `crates/app/src/control/summary.rs` 的摘要格式或 `scene::batch` 接受的欄位時要一起改它。`crates/app/tests/skill_examples.rs` 只驗證其中的 `apply` 例子能套用，摘要格式的描述沒有測試。
```

- [ ] **Step 4：全部檢查並 commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add skills crates/app/tests/skill_examples.rs CLAUDE.md
git commit -m "Add the napkin skill for Claude Code"
```

- [ ] **Step 5：機器設定**

這一步改的是使用者機器上的設定，不在 repo 裡。

```bash
ln -sfn ~/Projects/napkin/skills/napkin ~/.claude/skills/napkin
ln -sfn ~/Projects/napkin/target/release/napkin ~/.local/bin/napkin
cargo build --release -p app
command -v napkin && napkin status; echo "exit=$?"
```

Expected: `command -v` 印出 `~/.local/bin/napkin`；napkin 沒開時 `status` 印 not running、`exit=1`。

`~/.config/hypr/windows.lua`：螢幕邏輯尺寸 2304×1440（2880×1800、縮放 1.25，上方保留 30）。napkin 目前 1300×900 置中，左右各只剩約 500 px，放不下終端機。改成 napkin 靠左、終端機在右邊：

```lua
-- napkin 開出來就是 SUPER+O（omarchy-hyprland-window-pop）的狀態：浮動、1300×900、
-- 釘在所有工作區、帶 pop tag（圓角 8）。靠左放，右邊留給 Ctrl+K 開的 Claude Code 終端機。
o.window("^napkin$", { float = true, size = { 1300, 900 }, move = { 40, "(monitor_h*0.5-450)" }, pin = true, tag = "+pop" })

-- napkin 的 Ctrl+K：Claude Code 終端機，浮在 napkin 右邊、同高、不重疊。
o.window("^org\\.napkin\\.agent$", { float = true, size = { 900, 900 }, move = { 1360, "(monitor_h*0.5-450)" }, pin = true, tag = "+pop" })
```

`move` 的語法以這台 Hyprland 0.56 lua 設定實際接受的為準：先讀 `~/.config/hypr/` 裡其他 `move` 用法或 omarchy 預設（`/usr/share/omarchy/default/hypr/`）確認寫法。存檔後 `hyprctl configerrors` 必須是空的。用 `hyprctl clients -j` 確認使用者目前開著的 napkin 不受影響（規則只套用在新開的視窗）。

- [ ] **Step 6：交給使用者的人工驗收**

回報裡列出這些步驟，由使用者操作（AI spec §6）：

1. 關掉 napkin 再按 SUPER+N，確認 napkin 在左邊、右上角與標題是完整路徑，點路徑後貼上是絕對路徑。
2. 在 napkin 按 `Ctrl+K`，右邊開出終端機並啟動 Claude Code（`--continue` 接回 `~/Documents/napkin` 的上一段對話，第一次是新對話）；再按一次 `Ctrl+K` 只會聚焦它。
3. 請 Claude 畫一張五個方塊、四條箭頭的流程圖：圖在 napkin 裡分批出現，完成後 `Ctrl+Z` 一批一批復原。
4. 自己手畫一張草圖、選取它，請 Claude 說明看到什麼。
5. 把 Claude 畫的檔案拖進 excalidraw.com：元件、標籤、箭頭綁定都正確，拖動形狀時箭頭跟著走。
6. 按住 SUPER 再按 H、A、R 等沒有綁定的字母，napkin 的工具不會被切掉。

---

## 自我檢查

- AI spec §3.1 socket（0600、背景執行緒、channel、`request_repaint`、佔用與殘留、唯讀）：Task 7、8。
- §3.2 六個子指令與 not running 訊息：Task 5 到 7。
- §3.3 skill 位置、symlink、內容：Task 11。
- §3.4 `Ctrl+K`、聚焦、Hyprland 規則：Task 9、11。
- §3.5 路徑顯示、點擊複製、視窗標題：Task 9。
- §4.1 整批驗證、一步 undo、使用者刪掉元件時整批失敗：Task 3、4。
- §4.2 skeleton 支援範圍與拒絕的寫法、代號：Task 2。
- §4.3 update／delete、`Raw` 限制、`bump_version` 只在有變時：Task 3。
- §4.4 回應與 `revision`：Task 5。
- §4.5 文字量測、不換行、不撐大：Task 1、2（警告是與 spec 不同的地方 7）。
- §4.6 箭頭綁定的欄位與 `boundElements`：Task 2。
- §6 單元測試、整合測試、JS 基準、GPU 測試、人工驗收：各任務與 Task 11 Step 6。
- m4a-deferred 的「要先處理的」三項：Task 4（history diff）、Task 10（Super 鍵、刪檔後唯讀）。
