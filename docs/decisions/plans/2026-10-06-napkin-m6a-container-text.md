# napkin M6a：容器文字換行與長高 Implementation Plan

> Historical record, frozen 2026-10-06. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 容器文字照 Excalidraw 自動換行、放不下時容器長高；容器縮放時重新換行並有最小尺寸；獨立文字可以用左右邊縮放成固定寬度並換行。編輯器、AI batch 介面、屬性面板、貼上都走同一套規則。

**Architecture:** 兩個新的 `scene` 模組。`text_wrap` port `textWrapping.ts`（斷詞與換行，字寬由 `TextMeasure` 注入）。`bound_text` port `textElement.ts` 的 `redrawTextBoundingBox`、`handleBindTextResize`、`computeContainerDimensionForBoundText`，並收納目前散在 `transform.rs` 的 `bound_text_max_size`／`bound_text_position`。現有的「置中、不換行、放不下就溢出」呼叫點（`batch::add::bind_label`、`batch` 的 update、`editor::properties::redraw_text`、`editor::text::commit_text`、`transform` 的縮放）全部改呼叫這兩個函數。縮放手勢需要量字，所以 `Editor::pointer_move`／`pointer_up` 多一個 `&mut dyn TextMeasure` 參數。`app` 端把量字器接上，文字編輯疊層照容器寬度換行。

**Tech Stack:** 沿用 M4b：Rust 1.98.1（edition 2024）、eframe／egui 0.36.2、serde_json 1、regex 1。新增依賴 `unicode-normalization`（只給 `scene`，`parseTokens` 的 NFC 正規化）。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（主 spec §5.8「容器文字」、§7.3、§9.2）；`docs/decisions/specs/2026-09-26-napkin-ai-design.md`（AI spec §4.5，「自動換行、容器自動長高仍然在原本排定的里程碑」指的就是這個里程碑）。

**前置：** 在 `master`（`0e2cfee`）開出的分支 `m6a-container-text` 上工作，在原本的 checkout，不開 worktree。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號或計畫的決定編號（可以引用「spec §N」與 Excalidraw 函數名稱）。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 行為以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`。port 時看 JS 原始碼，不看計畫的摘要；兩者不一致時照 JS，並在回報寫出差異。
- `scene` 與 `rough` 不能依賴 egui、wgpu、glyphon。字寬一律由 `scene::text::TextMeasure` 注入。
- JS 數值語意走 `rough::js`（`Math.round` 用 `math_round`，`Math.ceil` 與 `f64::ceil` 相同）；亂數與時間走 `scene::env::Env`。JS 的 `\s` 不等於 `regex` 的 `\s`，用 `color.rs` 已有的 JS 空白定義。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫。
- `Element::Raw` 只能搬移、刪除、改 `index`。Raw 容器（例如 stickynote）的標籤不換行、不長高，維持現狀。
- 箭頭容器（箭頭標籤）不在這個里程碑：`redraw_text_bounding_box` 與 `handle_bind_text_resize` 遇到箭頭容器時什麼都不做，箭頭標籤的換行與定位屬於 M6b。
- 每個會改元件的使用者動作是一步 undo（主 spec §5.7）。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試沒有 adapter 時直接失敗，不跳過。改了 `tools/baseline/scene/cases.mjs` 或 `generate.mjs` 的任務要執行 `cd tools/baseline && npm run scene` 並 commit 產出的 JSON，其他群組的 JSON 必須完全不變。
- GUI 煙霧測試只做開視窗、截圖、關閉；用自己啟動的行程 PID 找視窗，`kill -TERM <pid>` 關閉；保留真實的 `WAYLAND_DISPLAY`／`XDG_RUNTIME_DIR`，只換 `HOME`；視窗只開幾秒。不用 `wtype`，不送任何按鍵或滑鼠事件。

## 與 spec 不同的地方

1. **M6 拆成兩半。** 原本的 M6 是「綁定跟隨與容器文字」。看過 JS 之後，箭頭綁定除了跟隨，還包含編輯器裡根本還沒做的「拖箭頭端點到形狀上建立綁定」（`getBindingStrategyForDraggingBindingElementEndpoints_simple`、`projectFixedPointOntoDiagonal`、中點吸附）、外框交點的曲線求解（`intersectElementWithLineSegment`）、箭頭標籤沿路徑定位，份量比容器文字大好幾倍。容器文字先做（M6a），因為綁定跟隨時箭頭標籤要用 `handleBindTextResize` 重新換行，依賴這裡的換行。M6b（箭頭綁定：建立、跟隨、標籤定位、載入修復）另寫計畫。
2. **編輯中不即時長高。** Excalidraw 的 `textWysiwyg` 在打字時就讓容器長高、刪字時縮回原高度。napkin 照主 spec §7.3 在編輯結束才寫回元件，所以 commit 時做一次 `redrawTextBoundingBox`：只會長高，不會縮。打字時疊層照容器的最大寬度換行（egui 自己的斷行，只是預覽），可能暫時超出容器下緣。
3. **最小容器寬度用固定字元集量。** `getApproxMinLineWidth` 取 `charWidth` 快取裡量過的最大字寬，快取內容取決於這個 session 之前換行過哪些字，不可重現。napkin 一律走它的 fallback 分支：`DUMMY_TEXT`（A 到 Z、0 到 9）每個字元的最大寬度加上 `BOUND_TEXT_PADDING * 2`。
4. **`charWidth` 不快取。** JS 以 `charCodeAt(0)` 為鍵快取單字寬度，astral 字元（emoji）共用高位 surrogate 時會互相蓋掉。napkin 每次都呼叫 `TextMeasure::line_width`，不重現這個碰撞。

## 寫計畫時做的決定

1. **基準。** 新增兩個 scene 基準群組，由 `generate.mjs` 跑打包好的 Excalidraw 原始碼，字寬用既有的 `CharWidthMeasure` 公式（每個 UTF-16 code unit 為 `0.6 * fontSize`）：`text_wrap`（`parseTokens`、`wrapText`）與 `bound_text`（`redrawTextBoundingBox`、`handleBindTextResize`，經 `Scene` 執行，比對容器與文字的 `x`、`y`、`width`、`height`、`text`）。既有的 `skeleton` 群組加入會換行、會長高的標籤案例，驗證 AI batch 的路徑。
2. **斷詞的寫法。** `regex` crate 沒有 lookaround，`parseTokens` 不能照抄成一條 regex。照 ECMAScript `@@split` 的演算法逐位置嘗試比對：先試 emoji 那一支（沒有 lookaround，可以用 `regex` 錨定在該位置比對），再用前後字元的類別判斷七條零寬斷點規則。字元類別（`\p{Script=Han}` 等）用 `regex` 的單字元比對。只 port `getLineBreakRegexAdvanced`，`Simple` 版是給不支援 lookbehind 的瀏覽器用的。
3. **量字器以參數傳入。** 沿用既有的寫法（`set_property`、`commit_text`、`paste`、`apply_batch` 都收 `&mut dyn TextMeasure`）：`Editor::pointer_move` 與 `Editor::pointer_up` 多一個 `measure` 參數。`app` 傳入一個延遲解析的轉接器，只有真的量字時才等 `FontMeasure` 建好，`--bench` 拖動不會因此同步建字型系統。
4. **AI batch 的 `warnings` 拿掉。** 換行加長高之後標籤一定放得下，「標籤放不下」是唯一一種 warning。`BatchReport::warnings`、回應的 `warnings` 欄位、skill 裡的相關說明一起刪除。
5. **AI 看到的文字是 `originalText`。** 換行後 `text` 含有軟換行；`napkin` 摘要（`control/summary.rs`）改輸出 `originalText`（沒有時用 `text`），AI 讀到的是它自己寫的字。

## 檔案結構

```
crates/scene/src/
  text_wrap.rs          parseTokens、wrapText（textWrapping.ts）
  bound_text.rs         容器標籤：最大尺寸、定位、redrawTextBoundingBox、handleBindTextResize、最小尺寸
  transform.rs          縮放改用 bound_text；文字 E/W 縮放
  batch/add.rs、batch/mod.rs   標籤換行長高，拿掉 warnings
  editor/mod.rs、editor/select.rs、editor/text.rs、editor/properties.rs   量字器參數、commit／屬性／貼上走 redraw
crates/scene/tests/
  baseline.rs（text_wrap、bound_text 兩個測試）、editor_text.rs、editor_select.rs、editor_batch.rs
crates/app/src/
  napkin_app.rs         延遲量字器、pointer 事件傳入
  text_edit.rs          疊層換行寬度
  control/summary.rs、control/handler.rs   originalText、拿掉 warnings
skills/napkin/SKILL.md
tools/baseline/scene/   text_wrap、bound_text 群組與 skeleton 新案例
```

---

### Task 1：`text_wrap`（斷詞與換行）

**Files:**
- Create: `crates/scene/src/text_wrap.rs`
- Modify: `crates/scene/src/lib.rs`（`pub mod text_wrap;`）、`crates/scene/Cargo.toml`、`Cargo.toml`（workspace 依賴 `unicode-normalization = "0.1"`）、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/text_wrap.json`

**Interfaces:**
- Consumes: `scene::text::TextMeasure`（`line_width(line, font_family, font_size)`）。
- Produces:

```rust
// scene::text_wrap
/// `parseTokens`: `line` (no `\n`) NFC-normalized and split at every line break opportunity
/// of `getLineBreakRegexAdvanced`; empty pieces dropped.
pub fn parse_tokens(line: &str) -> Vec<String>;

/// `wrapText` / `getWrappedTextLines(...).map(text).join("\n")`. A non-finite or negative
/// `max_width` splits only on existing `\n`.
pub fn wrap_text(
    text: &str,
    font_family: f64,
    font_size: f64,
    max_width: f64,
    measure: &mut dyn TextMeasure,
) -> String;
```

要讀的 JS：`packages/element/src/textWrapping.ts` 全檔（`COMMON`、`CJK`、`EMOJI`、`getLineBreakRegexAdvanced`、`getEmojiRegexUnicode`、`Break`、`parseTokens`、`getWrappedTextLines`、`wrapLine`、`wrapWord`、`trimLine`、`trimLineEndAtSoftBreak`、`isSingleCharacter`）；`textMeasurements.ts` 的 `getLineWidth`、`charWidth`。

實作要點（以 JS 為準）：

- `parse_tokens` 照 ECMAScript `String.prototype.split(regex)`：`p` 為上次切點，`q` 從 `p` 開始；在 `q` 嘗試比對，失敗就前進一個 code point；成功且結尾 `e == p` 也前進；否則推入 `S[p..q]`、推入擷取到的 emoji（零寬規則沒有擷取），`p = q = e`。最後推入 `S[p..]`，丟掉空字串。
- 在位置 `q`（前一個字元 `a`、後一個字元 `b`，字串頭沒有 `a`、字串尾沒有 `b`）依序試：emoji（錨定比對 `getEmojiRegexUnicode` 的 pattern，`\p{RI}`、`\p{Emoji_Modifier}`、`\p{Extended_Pictographic}`、`\p{Emoji_Presentation}`、`\p{Emoji}` 用 `regex` 的 Unicode 屬性），然後七條零寬規則，每條的前後條件照 `Break.*` 組出的 lookbehind／lookahead。lookbehind 在沒有 `a` 時不成立；negative lookahead 在沒有 `b` 時成立。
- `wrapLine` 對單一 code point 的 token 用「目前行寬 + 該字寬」，其他 token 量整行，與 JS 相同。
- `/\s/` 一律用 JS 的空白定義。

- [ ] **Step 1：失敗的單元測試**（`text_wrap.rs` 的 `#[cfg(test)]`，`CharWidthMeasure` 在 fontSize 20 時每字 12）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::CharWidthMeasure;

    fn wrap(text: &str, max_width: f64) -> String {
        wrap_text(text, 5.0, 20.0, max_width, &mut CharWidthMeasure)
    }

    #[test]
    fn breaks_at_spaces_and_drops_the_space_at_the_soft_break() {
        assert_eq!(wrap("hello world", 70.0), "hello\nworld");
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_between_characters() {
        assert_eq!(wrap("abcdefghij", 50.0), "abcd\nefgh\nij");
    }

    #[test]
    fn trailing_spaces_on_the_last_line_are_kept_only_while_they_fit() {
        assert_eq!(wrap("ab   ", 40.0), "ab ");
    }

    #[test]
    fn cjk_breaks_between_any_two_characters() {
        assert_eq!(wrap("中文字", 30.0), "中文\n字");
    }

    #[test]
    fn hard_breaks_survive_and_a_bad_width_only_splits_on_them() {
        assert_eq!(wrap("a\nb", 100.0), "a\nb");
        assert_eq!(wrap("hello world", f64::NAN), "hello world");
        assert_eq!(wrap("hello world", -1.0), "hello world");
    }

    #[test]
    fn hyphen_breaks_after_and_whitespace_is_its_own_token() {
        assert_eq!(parse_tokens("Hello-world"), ["Hello-", "world"]);
        assert_eq!(parse_tokens("a  b"), ["a", " ", " ", "b"]);
    }
}
```

Run: `cargo test -p scene text_wrap`
Expected: 編譯失敗（模組不存在）。

- [ ] **Step 2：實作 `text_wrap.rs`，單元測試通過。**

- [ ] **Step 3：基準群組 `text_wrap`**

`generate.mjs` 的 bundle 入口加上 `export { wrapText, parseTokens } from "@excalidraw/element/textWrapping";`，並在已有的 `setCustomTextMetricsProvider` 之後寫出群組。`cases.mjs` 新增：

```js
/**
 * [name, call, args] for the text_wrap group. `wrapText` args are
 * [text, fontSize, fontFamily, maxWidth]; the font string is built with `getFontString`.
 * `parseTokens` takes one line.
 */
export const textWrapCases = [
  ["tokensLatin", "parseTokens", ["Hello-world, this is (a) test."]],
  ["tokensCjkPunctuation", "parseTokens", ["Hello 「世界。」🌎🗺"]],
  ["tokensKorean", "parseTokens", ["Hello(한글)"]],
  ["tokensCurrency", "parseTokens", ["Price￥100 and $5"]],
  ["tokensEmojiSequences", "parseTokens", ["👨‍👩‍👧‍👦 👍🏽 ☂️ 1️⃣ 🇨🇿🇯🇵 🏳️‍🌈"]],
  ["tokensMixed", "parseTokens", ["日本語のテキスト、English words-with-hyphens！"]],
  ["tokensDecomposed", "parseTokens", ["českyで"]],
  ["wrapLatin", "wrapText", ["The quick brown fox jumps over the lazy dog", 20, 5, 150]],
  ["wrapLongWord", "wrapText", ["Supercalifragilisticexpialidocious", 20, 5, 100]],
  ["wrapTrailingSpaces", "wrapText", ["ab      ", 20, 5, 40]],
  ["wrapHardBreaks", "wrapText", ["first line\n\nthird line is long", 16, 6, 90]],
  ["wrapCjk", "wrapText", ["這是一段很長的中文句子，需要換行。", 20, 5, 100]],
  ["wrapEmoji", "wrapText", ["hi 👨‍👩‍👧‍👦👨‍👩‍👧‍👦 there", 20, 5, 30]],
  ["wrapExactFit", "wrapText", ["abc def", 20, 5, 84]],
  ["wrapNarrowerThanAChar", "wrapText", ["abc", 20, 5, 5]],
  ["wrapNaN", "wrapText", ["a b c", 20, 5, NaN]],
  ["wrapNegative", "wrapText", ["a b c", 20, 5, -1]],
];
```

產生器對每個案例呼叫對應函數（`wrapText(text, getFontString({fontSize, fontFamily}), maxWidth)`），`args` 照原樣寫進 JSON（NaN 由 harness 編碼）。`getFontString` 從 `@excalidraw/common` 匯出。`crates/scene/tests/baseline.rs` 新增：

```rust
#[test]
fn text_wrap() {
    check_group(&dir(), "text_wrap", |case| match case.call.as_str() {
        "parseTokens" => json!(scene::text_wrap::parse_tokens(
            case.args[0].as_str().expect("line")
        )),
        "wrapText" => json!(scene::text_wrap::wrap_text(
            case.args[0].as_str().expect("text"),
            testkit::number(&case.args[2]),
            testkit::number(&case.args[1]),
            testkit::number(&case.args[3]),
            &mut CharWidthMeasure,
        )),
        other => panic!("unknown call {other}"),
    });
}
```

（`check_group` 回呼的參數型別、`case.call`／`case.args` 的實際名稱與數字轉換函數照 `testkit` 與 `baseline.rs` 既有的群組寫法調整。）

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline text_wrap`
Expected: PASS；`git diff --stat crates/scene/tests/baseline` 只有新增的 `text_wrap.json`。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add Cargo.toml Cargo.lock crates/scene tools/baseline/scene
git commit -m "Port Excalidraw's text wrapping"
```

---

### Task 2：`bound_text`（換行、長高、縮放時重新換行）

**Files:**
- Create: `crates/scene/src/bound_text.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/transform.rs`（`bound_text_max_size`、`bound_text_position` 搬到 `bound_text`，`transform` 與其他呼叫端改用新位置）、`crates/scene/src/batch/add.rs`、`crates/scene/src/batch/mod.rs`、`crates/scene/src/editor/properties.rs`、`crates/scene/src/editor/text.rs`（只改 import）、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/bound_text.json`

**Interfaces:**
- Consumes: Task 1 的 `text_wrap::wrap_text`；`text::measure_text`、`text::line_height`、`text::normalize_text`；`transform::HandleKind`。
- Produces:

```rust
// scene::bound_text
/// `BOUND_TEXT_PADDING`.
pub const BOUND_TEXT_PADDING: f64 = 5.0;

/// `getBoundTextMaxWidth`/`getBoundTextMaxHeight` for a rectangle, diamond or ellipse
/// container; `None` for any other element.
pub fn bound_text_max_size(container: &Element) -> Option<[f64; 2]>;

/// `computeBoundTextPosition` for a rectangle, diamond or ellipse container.
pub fn bound_text_position(container: &Element, text: &TextElement) -> Option<[f64; 2]>;

/// `computeContainerDimensionForBoundText` for napkin's three container kinds (`kind` is the
/// element type string).
pub fn container_dimension_for_bound_text(dimension: f64, kind: &str) -> f64;

/// `getApproxMinLineWidth` through its measure-`DUMMY_TEXT` fallback (always taken here, see
/// this module's doc comment) and `getApproxMinLineHeight`: `[width, height]`.
pub fn approx_min_container_size(
    font_family: f64,
    font_size: f64,
    line_height: f64,
    measure: &mut dyn TextMeasure,
) -> [f64; 2];

/// `getMinTextElementWidth`.
pub fn min_text_element_width(
    font_family: f64,
    font_size: f64,
    line_height: f64,
    measure: &mut dyn TextMeasure,
) -> f64;

/// `redrawTextBoundingBox(text, container)`: rewraps `originalText` (to the container's max
/// width, or to the text's own width when it is a standalone text with `autoResize: false`),
/// remeasures, grows a rectangle/diamond/ellipse container that is now too small, and
/// recenters a bound text. `container` is the container's position, if any; an arrow or
/// `Raw` container leaves everything untouched. Bumps every element that changed. Returns
/// whether anything changed.
pub fn redraw_text_bounding_box(
    file: &mut SceneFile,
    text: usize,
    container: Option<usize>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool;

/// `handleBindTextResize(container, handle, keepAspectRatio, fromCenter, flipByY)` after the
/// container at `container` was resized: rewraps its live bound text unless the handle is
/// `N`/`S` without `keep_aspect_ratio`, grows the container (anchored per the JS) when the
/// text no longer fits, and recenters the text. `handle` is `None` for a resize that came
/// from no handle (a batch `update`), which rewraps like a side handle. A no-op for a
/// container without a live typed text label. Returns whether anything changed.
#[expect(clippy::too_many_arguments, reason = "mirrors handleBindTextResize")]
pub fn handle_bind_text_resize(
    file: &mut SceneFile,
    container: usize,
    handle: Option<HandleKind>,
    keep_aspect_ratio: bool,
    from_center: bool,
    flip_by_y: bool,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool;
```

要讀的 JS：`packages/element/src/textElement.ts`（`redrawTextBoundingBox`、`handleBindTextResize`、`computeBoundTextPosition`、`getContainerCoords`、`computeContainerDimensionForBoundText`、`getBoundTextMaxWidth`、`getBoundTextMaxHeight`）；`textMeasurements.ts`（`measureText`、`getApproxMinLineWidth`、`getApproxMinLineHeight`、`getMinTextElementWidth`）；`sizeHelpers.ts` 的 `getPositionAfterHeightChange`。sticky note 分支與 `originalContainerCache` 不 port。

- [ ] **Step 1：失敗的單元測試**（`bound_text.rs` 的 `#[cfg(test)]`；fontSize 20、lineHeight 1.25 時每字寬 12、每行高 25）

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample::{self, CharWidthMeasure};

    struct FixedEnv;

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(0);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    /// A container at the origin with `kind`'s `[w, h]` and a centered label `"hello world"`
    /// (unwrapped, 132 wide, one line).
    fn labeled(kind: &str, size: [f64; 2]) -> SceneFile {
        sample::file(vec![
            sample::with(
                sample::generic(kind, "c", [0.0, 0.0, size[0], size[1]]),
                json!({"boundElements": [{"id": "t", "type": "text"}]}),
            ),
            sample::with(
                sample::text("t", [0.0, 0.0, 132.0, 25.0], "hello world", Some("c")),
                json!({"textAlign": "center", "verticalAlign": "middle"}),
            ),
        ])
    }

    fn rect(file: &SceneFile, index: usize) -> [f64; 4] {
        let p = file.elements[index].placement().expect("placement");
        [p.x, p.y, p.width, p.height]
    }

    fn text_of(file: &SceneFile) -> String {
        file.elements[1].to_value()["text"].as_str().unwrap().to_owned()
    }

    #[test]
    fn redraw_wraps_grows_the_container_and_recenters() {
        let mut file = labeled("rectangle", [100.0, 50.0]);
        assert!(redraw_text_bounding_box(&mut file, 1, Some(0), &mut CharWidthMeasure, &mut FixedEnv));
        assert_eq!(text_of(&file), "hello\nworld");
        // Two lines are 50 tall; max height was 50 - 10 = 40, so the container grows to 50 + 10.
        assert_eq!(rect(&file, 0), [0.0, 0.0, 100.0, 60.0]);
        assert_eq!(rect(&file, 1), [20.0, 5.0, 60.0, 50.0]);
        assert_eq!(file.elements[1].to_value()["originalText"], json!("hello world"));
    }

    #[test]
    fn an_ellipse_grows_by_its_own_formula() {
        let mut file = labeled("ellipse", [400.0, 40.0]);
        redraw_text_bounding_box(&mut file, 1, Some(0), &mut CharWidthMeasure, &mut FixedEnv);
        assert_eq!(text_of(&file), "hello world");
        // round((25 + 10) / sqrt(2) * 2) = 49.
        assert_eq!(rect(&file, 0)[3], 49.0);
    }

    #[test]
    fn a_fitting_label_changes_nothing_but_its_position() {
        let mut file = labeled("rectangle", [300.0, 100.0]);
        redraw_text_bounding_box(&mut file, 1, Some(0), &mut CharWidthMeasure, &mut FixedEnv);
        assert_eq!(rect(&file, 0), [0.0, 0.0, 300.0, 100.0]);
        assert_eq!(rect(&file, 1), [84.0, 37.5, 132.0, 25.0]);
    }

    #[test]
    fn side_resize_rewraps_and_grows_downward() {
        let mut file = labeled("rectangle", [100.0, 50.0]);
        assert!(handle_bind_text_resize(
            &mut file, 0, Some(HandleKind::E), false, false, false,
            &mut CharWidthMeasure, &mut FixedEnv,
        ));
        assert_eq!(text_of(&file), "hello\nworld");
        assert_eq!(rect(&file, 0), [0.0, 0.0, 100.0, 60.0]);
    }

    #[test]
    fn top_resize_grows_upward_and_keeps_the_text_unwrapped() {
        let mut file = labeled("rectangle", [300.0, 20.0]);
        handle_bind_text_resize(
            &mut file, 0, Some(HandleKind::N), false, false, false,
            &mut CharWidthMeasure, &mut FixedEnv,
        );
        assert_eq!(text_of(&file), "hello world");
        // 25 > 20 - 10: grows to 35, anchored at the bottom edge.
        assert_eq!(rect(&file, 0), [0.0, -15.0, 300.0, 35.0]);
    }

    #[test]
    fn standalone_fixed_width_text_wraps_to_its_own_width() {
        let mut file = sample::file(vec![sample::with(
            sample::text("t", [0.0, 0.0, 70.0, 25.0], "hello world", None),
            json!({"autoResize": false}),
        )]);
        redraw_text_bounding_box(&mut file, 0, None, &mut CharWidthMeasure, &mut FixedEnv);
        let v = file.elements[0].to_value();
        assert_eq!(v["text"], json!("hello\nworld"));
        assert_eq!(v["width"], json!(70.0));
        assert_eq!(v["height"], json!(50.0));
    }

    #[test]
    fn min_sizes_measure_the_widest_dummy_character() {
        // Every character is 12 wide here: 12 + 10, and one line 25 + 10.
        assert_eq!(approx_min_container_size(5.0, 20.0, 1.25, &mut CharWidthMeasure), [22.0, 35.0]);
        // measureText("") measures " ": 12 + 10.
        assert_eq!(min_text_element_width(5.0, 20.0, 1.25, &mut CharWidthMeasure), 22.0);
    }
}
```

（`sample::text` 的實際參數與預設欄位以 `sample.rs` 為準；它產生的元件 `textAlign` 預設是 `left`，所以測試用 `sample::with` 覆寫。）

Run: `cargo test -p scene bound_text`
Expected: 編譯失敗。

- [ ] **Step 2：實作 `bound_text.rs`，把 `bound_text_max_size`、`bound_text_position`、`BOUND_TEXT_PADDING` 從 `transform.rs` 搬過來，所有呼叫端改 import，單元測試通過。**

這一步只建立函數並搬移既有的兩個函數，batch、屬性面板、縮放的行為在 Task 3 到 Task 5 才換。

- [ ] **Step 3：基準群組 `bound_text`**

`generate.mjs` 的 bundle 入口加上 `export { redrawTextBoundingBox, handleBindTextResize } from "@excalidraw/element/textElement";`。`cases.mjs` 新增：

```js
/**
 * [name, elements, op] for the bound_text group. Each case builds a `Scene` from `elements`
 * (container first, then its label), then:
 * - `{redraw: textId}` runs redrawTextBoundingBox(text, container-or-null, scene);
 * - `{resize: {id, width, height, x?, y?}, handle, keepAspect, fromCenter, flipY}` mutates
 *   the container to the given geometry, then runs handleBindTextResize.
 * Output: `{id: {x, y, width, height, text?}}` for every element, in input order.
 */
export const boundTextCases = [ /* 見下方清單 */ ];
```

案例至少涵蓋：rectangle、diamond、ellipse 各一個需要換行並長高的 `redraw`；一個放得下、只重新定位的 `redraw`；`verticalAlign` 為 `top`、`bottom` 與 `textAlign` 為 `left`、`right` 各一；旋轉 0.5 rad 的容器；CJK 標籤；標籤含 `\n`；`autoResize: false` 的獨立文字；`resize` 用 `e`、`w`、`n`、`s`、`ne`、`sw` 各一，`n`／`s` 一個 `keepAspect: true`；`fromCenter: true` 與 `flipY: true` 各一。標籤元件用 `cases.mjs` 已有的 `label()` 產生，`originalText` 與 `text` 相同、`width`／`height` 用 `0.6 * fontSize * 字數` 與 `行數 * fontSize * lineHeight` 預先算好。

`crates/scene/tests/baseline.rs` 的 `bound_text` 測試照同樣的步驟：讀 `elements` 建 `SceneFile`，套用 `op`，把每個元件的 `x`、`y`、`width`、`height`（文字另加 `text`）輸出成同樣的 JSON。`handle` 字串對應 `HandleKind`。

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline bound_text`
Expected: PASS；其他群組的 JSON 不變。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene tools/baseline/scene
git commit -m "Port bound text wrapping and container growth"
```

---

### Task 3：AI batch 的標籤換行與長高

**Files:**
- Modify: `crates/scene/src/batch/add.rs`、`crates/scene/src/batch/mod.rs`、`crates/scene/tests/editor_batch.rs`、`crates/scene/tests/baseline.rs`（`skeleton` 群組不變，只需要新案例通過）、`tools/baseline/scene/cases.mjs`、`crates/app/src/control/handler.rs`、`crates/app/src/control/summary.rs`、`crates/app/tests/control_socket.rs`（若有檢查 `warnings`）、`skills/napkin/SKILL.md`
- Regenerate: `crates/scene/tests/baseline/skeleton.json`

**Interfaces:**
- Consumes: Task 2 的 `redraw_text_bounding_box`、`handle_bind_text_resize`。
- Produces: `BatchReport` 不再有 `warnings` 欄位；`bind_label` 不再收 `name` 與 `warnings` 參數：

```rust
pub(crate) fn bind_label(
    file: &mut SceneFile,
    container: usize,
    label: &LabelSpec,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> usize;
```

行為：

- `bind_label` 建立文字後呼叫 `redraw_text_bounding_box(file, label, Some(container), ...)`，取代原本的定位與 `warn_if_label_overflows`（刪除）。新容器在同一批裡被長高時，版本規則照 `convertToExcalidrawElements` 的基準。
- `update` 改容器的 `width`／`height` 時呼叫 `handle_bind_text_resize(file, container, None, false, false, false, ...)`；只改 `x`／`y` 時維持原本的重新定位；`set.text` 改標籤時呼叫 `redraw_text_bounding_box`。`reposition_label_and_warn` 改名為不含 warn 的版本或併入上述呼叫。
- `editor::text` 呼叫 `bind_label` 的地方跟著改簽名。
- `summary.rs` 輸出文字時用 `originalText`，沒有時用 `text`。
- `handler.rs` 的回應拿掉 `warnings`。
- `SKILL.md`：刪掉「napkin never grows a shape to fit its label」、`warnings` 的段落與例子；改寫成「標籤會在框內自動換行，放不下時框會往下長高；`\n` 仍然是強制換行」。摘要格式的說明若提到 `text=` 的內容，註明是原始文字（不含自動換行）。

`cases.mjs` 的 `skeletonBatches`：把開頭「Every label fits its container」的註解改成描述現況，新增案例：

```js
  ["labelsWrapAndGrow", [
    { type: "rectangle", id: "r", x: 0, y: 0, width: 100, height: 40, label: { text: "Parse the input file" } },
    { type: "diamond", id: "d", x: 200, y: 0, width: 120, height: 60, label: { text: "Is it valid?" } },
    { type: "ellipse", id: "e", x: 0, y: 200, width: 90, height: 50, label: { text: "這是很長的中文標籤" } },
  ]],
  ["arrowBetweenGrownBoxes", [
    { type: "rectangle", id: "a", x: 0, y: 0, width: 80, height: 30, label: { text: "first step here" } },
    { type: "rectangle", id: "b", x: 300, y: 0, width: 80, height: 30, label: { text: "second" } },
    { type: "arrow", x: 85, y: 15, points: [[0, 0], [210, 0]], start: { id: "a" }, end: { id: "b" } },
  ]],
```

- [ ] **Step 1：失敗的測試**（`crates/scene/tests/editor_batch.rs`）

```rust
#[test]
fn a_long_label_wraps_and_its_box_grows() {
    let mut editor = editor(vec![]);
    let report = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "rectangle", "id": "r", "x": 0, "y": 0,
                "width": 100, "height": 50, "label": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    let id = &report.created["r"];
    let container = editor.file().elements.iter().find(|e| e.id() == Some(id)).unwrap();
    assert_eq!(container.placement().unwrap().height, 60.0);
    let label = editor.file().elements[1].to_value();
    assert_eq!(label["text"], json!("hello\nworld"));
    assert_eq!(label["originalText"], json!("hello world"));
}

#[test]
fn narrowing_a_box_by_update_rewraps_its_label() {
    let mut editor = editor(vec![]);
    let report = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "rectangle", "id": "r", "x": 0, "y": 0,
                "width": 300, "height": 50, "label": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    let id = report.created["r"].clone();
    editor
        .apply_batch(
            &json!({"ops": [{"op": "update", "id": id, "set": {"width": 100}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid update");
    assert_eq!(editor.file().elements[1].to_value()["text"], json!("hello\nworld"));
    assert_eq!(editor.file().elements[0].placement().unwrap().height, 60.0);
}
```

Run: `cargo test -p scene --test editor_batch`
Expected: FAIL（標籤沒有換行）。

- [ ] **Step 2：實作；重新產生基準**

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test --workspace`
Expected: PASS；`skeleton.json` 只多了兩個新案例，其他群組不變。

- [ ] **Step 3：全部檢查並 commit**

```bash
git add crates skills tools/baseline/scene
git commit -m "Wrap batch labels and grow their containers"
```

---

### Task 4：縮放時重新換行、最小尺寸、文字左右邊縮放

**Files:**
- Modify: `crates/scene/src/transform.rs`、`crates/scene/src/editor/mod.rs`、`crates/scene/src/editor/select.rs`、`crates/scene/src/editor/create.rs`、`crates/scene/src/editor/erase.rs`（只有簽名）、`crates/scene/tests/support/mod.rs`、所有呼叫 `pointer_move`／`pointer_up` 的測試、`crates/app/src/napkin_app.rs`
- Test: `crates/scene/tests/editor_select.rs`

**Interfaces:**
- Consumes: Task 2 的 `handle_bind_text_resize`、`approx_min_container_size`、`min_text_element_width`；Task 1 的 `wrap_text`。
- Produces:

```rust
impl<E: Env> Editor<E> {
    pub fn pointer_move(&mut self, event: PointerEvent, measure: &mut dyn TextMeasure);
    pub fn pointer_up(&mut self, event: PointerEvent, measure: &mut dyn TextMeasure);
}

// scene::transform: resize_element 與 resize_elements 多一個 `measure: &mut dyn TextMeasure`
// 參數（放在 `env` 前面）。
```

行為（照 `resizeElements.ts`）：

- `resizeSingleElement`：容器有綁定文字且不是 `keep_aspect_ratio` 時，`nextWidth`／`nextHeight` 先夾到 `approx_min_container_size`；縮放後呼叫 `handle_bind_text_resize(file, pos, Some(handle), keep_aspect_ratio, from_center, flip_by_y)`，取代原本的 `reposition_bound_text`。`keep_aspect_ratio` 時標籤字級跟著縮放（`boundTextFont`），JS 有這段就照 port。
- `resizeSingleTextElement` 的 `E`／`W`：寬度夾到 `min_text_element_width`，`originalText` 用 `wrap_text` 換到新寬度，高度重量，`getResizedOrigin` 定位，`autoResize` 設為 `false`。
- `resizeMultipleElements`：每個有綁定文字的容器照 JS 呼叫 `handle_bind_text_resize`（`keep_aspect_ratio` 為 `true`）。
- `support::click`、`support::drag` 內部傳 `&mut CharWidthMeasure`；直接呼叫 `pointer_move`／`pointer_up` 的測試補上 `&mut CharWidthMeasure`。
- `napkin_app.rs`：新增一個延遲解析的轉接器，`line_width` 第一次被呼叫時才走 `resolve_measure`，pointer 事件把它傳進 `Editor`：

```rust
/// A `TextMeasure` that resolves the real `FontMeasure` (see `resolve_measure`) only once
/// something actually measures text, so pointer events that never resize a label never wait
/// on, or synchronously build, the font system.
struct DeferredMeasure<'a> {
    measure: &'a mut Option<FontMeasure>,
    measure_rx: &'a mut Option<mpsc::Receiver<FontMeasure>>,
    ctx: Option<&'a egui::Context>,
}

impl TextMeasure for DeferredMeasure<'_> {
    fn line_width(&mut self, line: &str, font_family: f64, font_size: f64) -> f64 {
        resolve_measure(self.measure, self.measure_rx, self.ctx)
            .line_width(line, font_family, font_size)
    }
}
```

（`resolve_measure` 的實際簽名與回傳型別以 `napkin_app.rs` 為準。）

- [ ] **Step 1：失敗的測試**（`crates/scene/tests/editor_select.rs`）

```rust
fn labeled_box(width: f64) -> Vec<Value> {
    vec![
        sample::with(
            solid("r", [0.0, 0.0, width, 50.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::with(
            sample::text("t", [0.0, 12.5, 132.0, 25.0], "hello world", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
    ]
}

#[test]
fn narrowing_a_container_rewraps_its_label_and_grows_it() {
    let mut e = editor(labeled_box(300.0));
    click(&mut e, at(150.0, 25.0));
    // Grab the right edge midpoint and pull it to x = 100.
    drag(&mut e, at(300.0, 25.0), [100.0, 25.0]);
    assert_eq!(element(&e, "t").to_value()["text"], json!("hello\nworld"));
    assert_eq!(rect_of(&e, "r"), [0.0, 0.0, 100.0, 60.0]);
    assert!(e.command(Command::Undo));
    assert_eq!(element(&e, "t").to_value()["text"], json!("hello world"));
    assert_eq!(rect_of(&e, "r"), [0.0, 0.0, 300.0, 50.0]);
}

#[test]
fn a_labeled_container_stops_at_its_minimum_width() {
    let mut e = editor(labeled_box(300.0));
    click(&mut e, at(150.0, 25.0));
    drag(&mut e, at(300.0, 25.0), [3.0, 25.0]);
    // approx_min_container_size with CharWidthMeasure: 12 + 10.
    assert_eq!(rect_of(&e, "r")[2], 22.0);
}

#[test]
fn a_text_side_handle_fixes_its_width_and_wraps() {
    let mut e = editor(vec![sample::text("t", [0.0, 0.0, 132.0, 25.0], "hello world", None)]);
    click(&mut e, at(60.0, 12.0));
    drag(&mut e, at(132.0, 12.5), [70.0, 12.5]);
    let v = element(&e, "t").to_value();
    assert_eq!(v["text"], json!("hello\nworld"));
    assert_eq!(v["autoResize"], json!(false));
    assert_eq!(v["width"], json!(70.0));
    assert_eq!(v["height"], json!(50.0));
}
```

（邊的抓取點用 `handle_at` 的側邊判定；若 `solid` 或 `sample::text` 的預設讓抓取點落在角落或框外，照 `corner_handle_resizes_and_shows_a_resize_cursor` 的做法微調座標，斷言的數值不變。）

Run: `cargo test -p scene --test editor_select`
Expected: 編譯失敗（`pointer_move` 參數數量）或 FAIL。

- [ ] **Step 2：實作 scene 端與測試呼叫端，`cargo test -p scene` 通過。**

- [ ] **Step 3：`app` 接上 `DeferredMeasure`，`cargo test --workspace` 通過；全部檢查並 commit**

```bash
git add crates
git commit -m "Rewrap bound text on resize and let text resize from its sides"
```

---

### Task 5：文字編輯、屬性面板、貼上走換行

**Files:**
- Modify: `crates/scene/src/editor/text.rs`、`crates/scene/src/editor/properties.rs`、`crates/scene/src/editor/mod.rs`（`paste`）
- Test: `crates/scene/tests/editor_text.rs`、`crates/scene/tests/editor_properties.rs`、`crates/scene/tests/editor_clipboard.rs`

**Interfaces:**
- Consumes: Task 2 的 `redraw_text_bounding_box`、`approx_min_container_size`、`bound_text_max_size`。
- Produces: `TextEditing` 新增一個欄位，`app` 的疊層在 Task 6 使用：

```rust
pub struct TextEditing {
    // ...既有欄位...
    /// Width the editor wraps at, in scene units: a container label's max width
    /// (`getBoundTextMaxWidth`), a fixed-width text's own width, `None` for text that grows
    /// with its content.
    pub wrap_width: Option<f64>,
}
```

行為：

- 開始編輯既有文字時，`TextEditing::text` 是 `originalText`（沒有時用 `text`），不是換行後的 `text`。
- 在沒有標籤的容器上開始新標籤（`startTextEditing` 的容器分支）：容器先長到 `approx_min_container_size` 的最小寬高，這個改變與之後的 commit 同屬一步 undo；放棄（空字串）時容器尺寸也一起還原。
- `commit_text`：新標籤、改既有標籤、改 `autoResize: false` 的獨立文字，寫回 `text`／`originalText` 後呼叫 `redraw_text_bounding_box`（新標籤經 `bind_label`，Task 3 已處理）。`autoResize: true` 的獨立文字維持原本的量測與左上角不動。
- `properties.rs` 的 `redraw_text` 改成呼叫 `redraw_text_bounding_box`（`actionProperties` 改字型、字級後的 `redrawTextBoundingBox`），換字型讓容器長高時，容器的版本也遞增。
- `paste`：貼上的元件中，綁定在容器上的文字照 `App.tsx` 的 `addElementsFromPasteOrLibrary` 呼叫 `redraw_text_bounding_box`（用 napkin 的量字器重新換行）。

- [ ] **Step 1：失敗的測試**

`crates/scene/tests/editor_text.rs`：

```rust
#[test]
fn a_new_label_wraps_inside_its_container_and_grows_it() {
    let mut e = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0])]);
    assert!(e.double_click(at(50.0, 25.0)));
    assert_eq!(e.text_editing().unwrap().wrap_width, Some(90.0));
    assert!(e.commit_text("hello world", &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["text"], json!("hello\nworld"));
    assert_eq!(e.file().elements[0].placement().unwrap().height, 60.0);
    assert!(e.command(Command::Undo));
    assert_eq!(e.file().elements[0].placement().unwrap().height, 50.0);
}

#[test]
fn editing_a_wrapped_label_starts_from_its_original_text() {
    let mut e = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0])]);
    e.double_click(at(50.0, 25.0));
    e.commit_text("hello world", &mut CharWidthMeasure);
    assert!(e.double_click(at(50.0, 30.0)));
    assert_eq!(e.text_editing().unwrap().text, "hello world");
}

#[test]
fn a_tiny_container_grows_to_the_minimum_before_editing() {
    let mut e = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0])]);
    assert!(e.double_click(at(5.0, 5.0)));
    // approx_min_container_size with CharWidthMeasure at fontSize 20: [22, 35].
    let p = e.file().elements[0].placement().unwrap();
    assert_eq!([p.width, p.height], [22.0, 35.0]);
    assert!(!e.commit_text("", &mut CharWidthMeasure));
    let p = e.file().elements[0].placement().unwrap();
    assert_eq!([p.width, p.height], [10.0, 10.0]);
}
```

`crates/scene/tests/editor_properties.rs`：

```rust
#[test]
fn a_bigger_font_rewraps_the_label_and_grows_the_container() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 200.0, 50.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::with(
            sample::text("t", [34.0, 12.5, 132.0, 25.0], "hello world", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
    ]);
    e.command(Command::SelectAll);
    // fontSize 36: 21.6 per character, so "hello world" (237.6) no longer fits the 190 max
    // width and wraps to two lines, 2 * 36 * 1.25 = 90 tall.
    assert!(e.set_property(Property::FontSize(36.0), &mut CharWidthMeasure));
    let label = element(&e, "t").to_value();
    assert_eq!(label["text"], json!("hello\nworld"));
    assert_eq!(rect_of(&e, "r")[3], 90.0 + 10.0);
}
```

（`Property` 的字級變體名稱與 `support` 的 `element`／`rect_of` 照既有程式碼；兩行在 fontSize 36、lineHeight 1.25 時高 90，容器長到 `ceil(90) + 10 = 100`。）

`crates/scene/tests/editor_clipboard.rs`：貼上一個 Excalidraw 剪貼簿 JSON，內含寬 100 的 rectangle 與一個 `text: "hello world"`、`width: 132` 的綁定標籤，斷言貼上後標籤的 `text` 是 `"hello\nworld"`、容器高度長到 60。剪貼簿 JSON 的組法照同檔既有的貼上測試。

Run: `cargo test -p scene --test editor_text --test editor_properties --test editor_clipboard`
Expected: FAIL。

- [ ] **Step 2：實作，全部檢查並 commit**

```bash
git add crates/scene
git commit -m "Wrap text on edit, font change and paste"
```

---

### Task 6：文字編輯疊層換行與人工驗收清單

**Files:**
- Modify: `crates/app/src/text_edit.rs`
- Modify: `docs/decisions/plans/2026-10-06-napkin-m6a-container-text.md` 不改；人工驗收清單寫在本任務的回報裡

**Interfaces:**
- Consumes: Task 5 的 `TextEditing::wrap_width`。

行為：

- `wrap_width` 為 `Some(w)` 時，`layouter` 的 `LayoutJob` 設 `wrap.max_width = w * zoom`（螢幕像素），`TextEdit` 的 `desired_width` 也設成這個寬度，文字照 egui 的斷詞折行；為 `None` 時維持現在的不折行。這只是編輯中的預覽，commit 後的換行以 `scene::text_wrap` 為準。
- 對齊與錨點照現有的 `layout`／`pivot`；容器標籤置中時，換行後的每一行在 `wrap_width` 內置中。

- [ ] **Step 1：失敗的測試**（`text_edit.rs` 的 `#[cfg(test)]`，照同檔 `typed_text_stays_on_one_line_instead_of_wrapping_every_character` 的寫法跑一個 egui frame）

```rust
#[test]
fn a_label_wraps_at_its_container_width() {
    let editing = TextEditing {
        wrap_width: Some(90.0),
        ..sample_editing("center")
    };
    let rect = show_once(&editing, "hello world hello world hello world");
    assert!(rect.width() <= 90.0 + 1.0, "{rect:?}");
    assert!(rect.height() > 20.0 * 1.25 * 1.5, "{rect:?}");
}
```

（`show_once` 是把既有測試裡「跑一個 frame、回傳 `TextEdit` 的矩形」的步驟抽成的 helper；既有測試改用它。）

Run: `cargo test -p napkin text_edit`
Expected: FAIL。

- [ ] **Step 2：實作；GUI 煙霧測試**

暫時 `HOME` 下放一個含長標籤矩形的 `.excalidraw`（例如 Task 3 測試的 `hello world` 換成一段 40 字的英文，矩形寬 120），啟動 release build，截圖確認標籤在框內換行、框已長高，`kill -TERM` 關閉。

- [ ] **Step 3：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Wrap the text editing overlay at the label's width"
```

- [ ] **Step 4：回報人工驗收清單**（使用者本人操作）

1. 在矩形、菱形、橢圓上雙擊，打一段比框寬的英文：打字時預覽在框寬內折行；結束編輯後框往下長高，文字置中。
2. 打一段中文（fcitx5）：在任意兩字之間換行，`。」` 等標點不會落在行首。
3. 拖容器右邊往內縮：文字跟著重新換行、框跟著長高；縮到最窄會停住，不會比一個字還窄。
4. 選取獨立文字，拖左右邊：寬度固定，文字在新寬度內換行；再拖上下角，字級等比例縮放。
5. 屬性面板把容器標籤的字級調到最大：框長高容納全部文字。`Ctrl+Z` 一次回到原狀。
6. 請 Claude 畫一個框、標籤寫一句長句子：標籤自動換行、框長高；`napkin` 摘要顯示的是原本那一句（沒有插入換行）。
7. 把 napkin 存的檔拖進 excalidraw.com：換行位置與框高相同（中文因字型不同可能差一兩個字，主 spec §1.2 的已知差異）。

---

## 自我檢查

- 主 spec §5.8 容器文字：雙擊輸入（M4b 已有）、置中（`bound_text_position`）、容器移動時文字跟著移動（M4a 已有）、縮放時重新換行（Task 4）、文字高度超過時長高（Task 2、3、5）、換行規則 port `textWrapping.ts` 含 CJK（Task 1）。
- 主 spec §9.2「綁定：縮放後容器文字的位置正確」：Task 2 的 `bound_text` 基準與 Task 4 的編輯器測試。箭頭端點的部分屬於 M6b。
- AI spec §4.5「自動換行、容器自動長高仍然在原本排定的里程碑」：Task 3。
- M4a 計畫延後到這裡的項目：容器文字重新換行、容器自動長高、有綁定文字時的最小尺寸、文字元件左右邊縮放（Task 2、4）。箭頭標籤定位與綁定跟隨留在 M6b。
- `CLAUDE.md` 要求改 `scene::batch` 接受的欄位或摘要格式時同步改 `SKILL.md`：Task 3。
