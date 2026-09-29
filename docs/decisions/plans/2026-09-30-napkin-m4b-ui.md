# napkin M4b：工具列、屬性面板、文字、複製貼上、圖層、橡皮擦 Implementation Plan

> Historical record, frozen 2026-09-30. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** napkin 有工具列與屬性面板、能建立與編輯文字（含中文輸入）、能複製／貼上／複製一份（與 excalidraw.com 互通）、能上下移一層、能用橡皮擦刪除，完成主 spec §1.1 的編輯範圍（綁定跟隨與換行除外）。

**Architecture:** 規則放在 `scene`：`editor::properties`（屬性面板的狀態與套用）、`zindex`（一層上下移）、`duplicate`（複製一份與貼上的 id 重建）、`clipboard`（Excalidraw 剪貼簿 JSON）、`editor::text`（文字編輯的 session）、`editor::erase`（橡皮擦手勢）。`Editor` 新增兩個工具（`Text`、`Eraser`）、幾個指令與方法，每個修改仍是一步 undo。`app` 用 egui 畫工具列、屬性面板、疊在元件上的 `TextEdit`，把剪貼簿事件接到 `Editor`，渲染時把正在編輯的文字藏起來、把待擦除的元件變淡。

**Tech Stack:** 沿用 M5：Rust 1.98.1（edition 2024）、eframe／egui 0.36.2、wgpu 30、glyphon 0.12.0、serde_json 1。不新增依賴。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（主 spec；§1.1、§5.7、§6.4、§7.1、§7.2、§7.3、§7.5、§9.2）。M4a 計畫 `docs/decisions/plans/2026-09-17-napkin-m4a-editing-core.md` 的「與 roadmap、spec 不同的地方」第 1 項定義 M4b 的範圍。

**前置：** M5（分支 `m5-ai-interface`）尚未合併，使用者人工驗收中。M4b 的分支 `m4b-ui` 從 `m5-ai-interface` 的 `e06f54c` 開出，疊在它上面；M5 合併後再把 `m4b-ui` rebase 到 `master`。在原本的 checkout 工作，不開 worktree。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號、計畫的決定編號或 spec 偏離編號（可以引用「spec §N」與 Excalidraw 函數名稱）。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 行為以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`。port 時看 JS 原始碼，不看計畫的摘要；兩者不一致時照 JS，並在回報寫出差異。
- `scene` 與 `rough` 不能依賴 egui、wgpu、glyphon。字寬一律由 `scene::text::TextMeasure` 注入。
- JS 數值語意走 `rough::js`；亂數與時間走 `scene::env::Env`。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫。
- `Element::Raw` 只能搬移（`x`、`y`）、刪除、改 `index`（圖層順序、貼上）。屬性面板對 `Raw` 元件不做任何事。
- 每個會改元件的使用者動作是一步 undo（主 spec §5.7）。只改選取不記。
- 快捷鍵照主 spec §7.1：文字 `T` `8`、橡皮擦 `E` `0`、複製／貼上／複製一份 `Ctrl+C` `Ctrl+V` `Ctrl+D`、上移／下移一層 `Ctrl+]` `Ctrl+[`。
- 屬性面板照主 spec §7.2：左側，有選取或正在使用工具（選取與手以外）時顯示；內容有線條色、填充色、填充樣式、線寬、線條樣式、潦草度、邊角、箭頭頭部、字型、字級、透明度；調色盤用 Excalidraw 的預設色票。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試沒有 adapter 時直接失敗，不跳過。改了 `tools/baseline/scene/cases.mjs` 或 generator 的任務要執行 `cd tools/baseline && npm run scene` 並 commit 產出的 JSON，其他群組必須完全不變。
- GUI 煙霧測試只做開視窗、截圖、關閉；用自己啟動的行程 PID 找視窗，`kill -TERM <pid>` 關閉；保留真實的 `WAYLAND_DISPLAY`／`XDG_RUNTIME_DIR`，只換 `HOME`；視窗只開幾秒（使用者正在用這台桌面，新開的 napkin 視窗會搶到焦點）。不用 `wtype`，不送任何按鍵或滑鼠事件。

## 與 spec 不同的地方

1. **不做容器文字的自動換行與容器長高**（主 spec §5.8 的容器文字屬於 M6）。在容器上雙擊可以輸入標籤，用 M5 的 `bind_label`：置中、不換行、放不下就溢出。
2. **`Esc` 結束文字編輯時保留文字**（Excalidraw `textWysiwyg` 的 `Escape` 也是 submit）。主 spec §7.5 說「取消」，指的是結束編輯這個狀態。
3. **不做複製成 PNG**（`Shift+Alt+C`，主 spec §7.4）與剪下（`Ctrl+X`）。PNG 屬於 M7 的匯出；剪下 spec 沒列。
4. **「邊角」對箭頭顯示成箭頭類型（直線／曲線）**，對應 Excalidraw 的 `currentItemArrowType` 的 `sharp`／`round`，不提供 `elbow`（主 spec §1.2）。
5. **屬性面板不含文字對齊**，照主 spec §7.2 的清單。新文字一律靠左。
6. **貼上純文字建立一個文字元件**（Excalidraw `pasteFromClipboard` 的行為），不做 URL／圖片／mermaid。

## 寫計畫時做的決定

1. **JS 基準涵蓋 `moveOneLeft`／`moveOneRight` 與 `duplicateElements`。** 兩者都是純陣列函數。`duplicateElements` 會產生 id 與 seed，沿用 M5 skeleton 群組的正規化（刪 `seed`、`versionNonce`，id 依輸出順序改名，參照一起改名）與 `runCase` 的 `exactDespiteRandom`。
2. **複製一份照 Excalidraw 插在原元件後面**（`type: "in-place"`），元件順序會變。`History` 以位置比對，插入點之後每個位置都會記一筆；undo／redo 仍然正確，代價是這一步的記憶體較大。
3. **貼上照 `addElementsFromPasteOrLibrary`**：貼上元件的外框中心對齊游標（沒有游標位置時用畫面中心），`randomizeSeed`，貼上後選取它們。
4. **剪貼簿只走純文字**：`Ctrl+C` 用 `egui::Context::copy_text` 寫入 `serializeAsClipboardJSON` 的字串；`Ctrl+V` 讀 `egui::Event::Paste`。excalidraw.com 讀寫的也是 `text/plain` 裡的這段 JSON。
5. **文字編輯是 `Editor` 裡的一個狀態**（`TextEditing`），不是手勢：開始編輯時 `Editor` 記下要編輯的對象與字型，`app` 顯示 `TextEdit`，結束時呼叫 `Editor::commit_text`，這時才寫回元件（主 spec §7.3），整個編輯是一步 undo。空字串的新文字不建立；既有文字改成空字串就刪除（`textWysiwyg` 的 `onSubmit`）。
6. **egui 字型**：把三個內建字型與系統的 Noto Sans CJK 註冊進 egui，`TextEdit` 才能顯示手寫字型與中文候選字。CJK 字型用 glyphon 的 `fontdb` 查詢家族名 `Noto Sans CJK TC`（找不到時試 `Noto Sans CJK SC`、`Noto Sans CJK JP`），查不到就不加，照常運作。
7. **待擦除的元件變淡、正在編輯的文字隱藏**：`CanvasFrame` 多兩個 `Arc<HashSet<String>>`（`faded`、`hidden`），`plan_frame` 讀它們。透明度在 `plan_frame` 逐個 draw 決定（`element_alpha`），不在網格快取裡，所以不需要讓快取失效。變淡的比例照 Excalidraw `ELEMENT_READY_TO_ERASE_OPACITY`（20%）。
8. **工具列的圖示用 egui painter 自己畫**（外框、菱形、橢圓、箭頭、線、筆、`T`、橡皮擦、手、游標），不加圖示字型或圖片。
9. **UI 上的點擊不能穿到畫布**：`edit_input::FrameInput` 多一個 `pointer_over_ui: bool`（`ctx.is_pointer_over_area()`），為真時不送 `Down` 給編輯器。

## 檔案結構

```
crates/scene/src/
  editor/mod.rs          Tool::{Text, Eraser}、新指令、新方法的入口
  editor/style.rs        ItemStyle + font_family、font_size
  editor/properties.rs   屬性面板的狀態（getFormValue）與套用（actionProperties 的 change*）
  editor/text.rs         TextEditing、開始與結束文字編輯
  editor/erase.rs        橡皮擦手勢（eraser/index.ts、handleEraser）
  zindex.rs              moveOneLeft／moveOneRight
  duplicate.rs           duplicateElements（in-place 與 everything）
  clipboard.rs           serializeAsClipboardJSON、解析貼上的文字
crates/scene/tests/
  editor_properties.rs   editor_text.rs   editor_clipboard.rs   editor_erase.rs
crates/app/src/
  toolbar.rs             工具列
  properties_panel.rs    屬性面板
  text_edit.rs           疊在畫布上的 TextEdit
  fonts.rs               把內建字型與 CJK 註冊進 egui
  edit_input.rs          新快捷鍵、pointer_over_ui
  render/gpu.rs、render/plan.rs   CanvasFrame/View 的 faded 與 hidden
  napkin_app.rs          接線
tools/baseline/scene/    zindex、duplicate 兩個群組
```

---

### Task 1：`ItemStyle` 字型欄位與屬性面板的狀態與套用

**Files:**
- Create: `crates/scene/src/editor/properties.rs`、`crates/scene/tests/editor_properties.rs`
- Modify: `crates/scene/src/editor/mod.rs`、`crates/scene/src/editor/style.rs`

**Interfaces:**
- Consumes: M5 的 `text::{TextMeasure, measure_text, normalize_text}`、`transform::bound_text_position`、`new_element::bump_version`。
- Produces:

```rust
// editor/style.rs：ItemStyle 新增
pub font_family: f64,   // DEFAULT_FONT_FAMILY 5
pub font_size: f64,     // DEFAULT_FONT_SIZE 20

// editor/properties.rs
/// One property-panel section (`SelectedShapeActions` in
/// `packages/excalidraw/components/Actions.tsx`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    StrokeColor, BackgroundColor, FillStyle, StrokeWidth, StrokeStyle, Roughness,
    Edges, ArrowType, Arrowheads, FontFamily, FontSize, Opacity,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Property {
    StrokeColor(String),
    BackgroundColor(String),
    FillStyle(String),
    StrokeWidth(StrokeWidth),
    StrokeStyle(String),
    Roughness(f64),
    Edges(EdgeStyle),
    ArrowType(ArrowType),
    StartArrowhead(Option<String>),
    EndArrowhead(Option<String>),
    FontFamily(f64),
    FontSize(f64),
    Opacity(f64),
}
/// What the panel shows: which sections, and each section's current value; `None` means the
/// selected elements disagree (`getFormValue` returning its default for mixed values).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelState {
    pub sections: Vec<Section>,
    pub stroke_color: Option<String>,
    pub background_color: Option<String>,
    pub fill_style: Option<String>,
    pub stroke_width: Option<StrokeWidth>,
    pub stroke_style: Option<String>,
    pub roughness: Option<f64>,
    pub edges: Option<EdgeStyle>,
    pub arrow_type: Option<ArrowType>,
    pub start_arrowhead: Option<Option<String>>,
    pub end_arrowhead: Option<Option<String>>,
    pub font_family: Option<f64>,
    pub font_size: Option<f64>,
    pub opacity: Option<f64>,
}

impl<E: Env> Editor<E> {
    /// Empty `sections` when the panel should be hidden: nothing selected and the tool is
    /// Selection, Hand or Eraser.
    pub fn panel(&self) -> PanelState;
    /// Applies `property` to every selected typed element that has that property (and to a
    /// container's bound text for stroke colour, font family and font size), as one history
    /// step, and to `style()` so new elements take it. Returns whether any element changed.
    pub fn set_property(&mut self, property: Property, measure: &mut dyn TextMeasure) -> bool;
}
```

要讀的 JS：`packages/excalidraw/components/Actions.tsx` 的 `SelectedShapeActions`（哪些 section 對哪些元件類型顯示）；`packages/element/src/comparisons.ts`（`hasBackground`、`hasStrokeWidth`、`hasStrokeStyle`、`canChangeRoundness`、`hasText` 等）；`packages/excalidraw/actions/actionProperties.tsx` 的 `changeProperty`、`getFormValue`、各個 `actionChange*`（`StrokeColor`、`BackgroundColor`、`FillStyle`、`StrokeWidth`、`Sloppiness`、`StrokeStyle`、`Opacity`、`FontSize`、`FontFamily`、`RoundnessType`/`Roundness`、`ArrowType`、`Arrowhead`）；`packages/common/src/constants.ts` 的 `FONT_SIZES`；`getDefaultRoundnessTypeForElement`。

規則要點（以 JS 為準）：

- section 的顯示：線條色一律；填充色與填充樣式只有 `hasBackground`（rectangle、diamond、ellipse、line（封閉時也算，照 `hasBackground`）、freedraw）；線寬、線條樣式照 `hasStrokeWidth`／`hasStrokeStyle`；潦草度照 `hasStrokeStyle` 的同一組；邊角照 `canChangeRoundness`（rectangle、diamond、ellipse、line）；箭頭類型與箭頭頭部只有 arrow；字型與字級只有 text（選取容器時看它的標籤）；透明度一律。沒有選取、使用建立工具時，照該工具會建立的類型決定。
- 值：所有相關元件相同就回那個值，否則 `None`；沒有選取時回 `ItemStyle` 的值。
- 套用：`changeProperty` 對選取的元件（`includeBoundTextElement: true`）逐一 `newElementWith`，值相同就不動、不 bump。字級與字型改變時照 `redrawTextBoundingBox` 的「不換行」部分：用 `measure_text` 重算寬高，容器的標籤用 `bound_text_position` 重新定位，獨立文字照 `actionChangeFontSize` 的做法保持左上角（`textAlign` 是 `left` 時；`center`／`right` 時保持對應的錨點）。邊角改變時，rectangle 用 `{type: 3}`、其他用 `{type: 2}`，`sharp` 用 `null`。
- 不論有沒有元件改變，都把值寫進 `ItemStyle`（`appState.currentItem*`）。

- [ ] **Step 1：失敗的測試**（`crates/scene/tests/editor_properties.rs`，沿用 `tests/support` 的 `editor`、`at`、`drag`）

```rust
mod support;

use scene::editor::{EdgeStyle, Property, Section, StrokeWidth, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;
use support::*;

#[test]
fn panel_is_hidden_with_nothing_selected_under_the_selection_tool() {
    let e = editor(vec![]);
    assert!(e.panel().sections.is_empty());
}

#[test]
fn creation_tools_show_their_own_sections_with_item_style_values() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Arrow);
    let panel = e.panel();
    assert!(panel.sections.contains(&Section::Arrowheads));
    assert!(!panel.sections.contains(&Section::BackgroundColor));
    assert_eq!(panel.end_arrowhead, Some(Some("arrow".to_string())));
    e.set_tool(Tool::Text);
    let panel = e.panel();
    assert!(panel.sections.contains(&Section::FontSize));
    assert_eq!(panel.font_size, Some(20.0));
}

#[test]
fn mixed_values_read_as_none_and_setting_one_is_one_undo_step() {
    let mut e = editor(vec![
        sample::with(sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]), json!({"strokeColor": "#e03131"})),
        sample::generic("ellipse", "b", [20.0, 0.0, 10.0, 10.0]),
    ]);
    e.command(scene::editor::Command::SelectAll);
    assert_eq!(e.panel().stroke_color, None);
    let revision = e.revision();
    assert!(e.set_property(Property::StrokeColor("#1971c2".into()), &mut CharWidthMeasure));
    assert_eq!(e.revision(), revision + 1);
    assert_eq!(e.panel().stroke_color, Some("#1971c2".into()));
    assert_eq!(e.style().stroke_color, "#1971c2");
    assert!(e.command(scene::editor::Command::Undo));
    assert_eq!(e.panel().stroke_color, None);
}

#[test]
fn setting_the_current_value_changes_nothing_but_still_updates_item_style() {
    let mut e = editor(vec![sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0])]);
    e.command(scene::editor::Command::SelectAll);
    let before = e.file().clone();
    assert!(!e.set_property(Property::StrokeWidth(StrokeWidth::Medium), &mut CharWidthMeasure));
    assert_eq!(e.file(), &before);
    e.set_property(Property::Edges(EdgeStyle::Sharp), &mut CharWidthMeasure);
    assert_eq!(e.file().elements[0].to_value()["roundness"], json!(null));
    assert_eq!(e.style().edges, EdgeStyle::Sharp);
}

#[test]
fn font_size_remeasures_text_and_recenters_labels() {
    let mut e = editor(vec![
        sample::with(sample::generic("rectangle", "r", [0.0, 0.0, 200.0, 100.0]),
                     json!({"boundElements": [{"id": "t", "type": "text"}]})),
        sample::with(sample::text("t", [76.0, 37.5, 48.0, 25.0], "box", Some("r")),
                     json!({"textAlign": "center", "verticalAlign": "middle"})),
    ]);
    e.command(scene::editor::Command::SelectAll);
    assert!(e.set_property(Property::FontSize(40.0), &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["fontSize"], json!(40.0));
    assert_eq!(label["width"], json!(3.0 * 40.0 * 0.6));
    assert_eq!(label["height"], json!(50.0));
    assert_eq!(label["x"], json!(5.0 + (190.0 / 2.0 - 72.0 / 2.0)));
    assert_eq!(label["y"], json!(5.0 + (90.0 / 2.0 - 25.0)));
}

#[test]
fn raw_elements_are_left_alone() {
    let mut e = editor(vec![json!({"id": "i", "type": "image", "x": 0, "y": 0, "width": 5,
        "height": 5, "isDeleted": false, "version": 1, "versionNonce": 1, "strokeColor": "#000"})]);
    e.command(scene::editor::Command::SelectAll);
    let before = e.file().clone();
    assert!(!e.set_property(Property::StrokeColor("#e03131".into()), &mut CharWidthMeasure));
    assert_eq!(e.file(), &before);
}
```

（`Tool::Text` 在 Task 5 才有實際行為；這一步在 `Tool` 加上 `Text` 與 `Eraser` 兩個變體，`create::pointer_down` 等照 `Selection | Hand` 一樣忽略它們，`set_tool` 照建立工具清除選取。Task 5、6 再實作。）

Run: `cargo test -p scene --test editor_properties`
Expected: 編譯失敗。

- [ ] **Step 2：實作 `properties.rs` 與 `ItemStyle` 欄位**

照上面的規則實作；`Editor` 的 `set_property` 用既有的 `clone_scene` 與 `finish_edit` 記錄一步。`ItemStyle` 的新欄位加到 `Default`。`properties.rs` 的模組 doc comment 列出 port 的 JS 函數與 commit。

Run: `cargo test -p scene`
Expected: PASS。

- [ ] **Step 3：全部檢查並 commit**

```bash
git add crates/scene
git commit -m "Add the property panel state and property changes to the editor"
```

---

### Task 2：圖層上下移（`Ctrl+]`／`Ctrl+[`）

**Files:**
- Create: `crates/scene/src/zindex.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/editor/mod.rs`、`crates/scene/tests/baseline.rs`、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`；新增 `crates/scene/tests/baseline/zindex.json`
- Test: `crates/scene/tests/editor_select.rs`（加一個互動測試）

**Interfaces:**
- Produces:

```rust
// scene::zindex
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction { Left, Right }
/// `moveOneLeft` / `moveOneRight` (`packages/element/src/zindex.ts`): the reordered elements,
/// indices synced with `syncMovedIndices` for the moved ones. `None` when nothing moves.
pub fn move_one(elements: &[Element], selected: &Selection, direction: Direction,
                env: &mut impl Env) -> Option<Vec<Element>>;

// scene::editor::Command 新增
SendBackward,  // Ctrl+[
BringForward,  // Ctrl+]
```

要讀的 JS：`packages/element/src/zindex.ts` 全檔（`getIndicesToMove`、`toContiguousGroups`、`getTargetIndex`、`getTargetIndexAccountingForBinding`、`shiftElementsByOne`、`moveOneLeft`、`moveOneRight`；frame 與 editing group 的分支照 port，napkin 沒有 editing group，傳 `null`）；`packages/excalidraw/actions/actionZindex.tsx` 的 `keyTest`；`fractionalIndex.ts` 的 `syncMovedIndices`（M2 已 port）。

- [ ] **Step 1：JS 基準**

`cases.mjs` 加：

```js
/** [case name, elements, selected ids, "left" | "right"] for moveOneLeft/moveOneRight. */
export const zindexCases = [
  ["right/middle", ["a", "b", "c"], ["b"], "right"],
  ["left/middle", ["a", "b", "c"], ["b"], "left"],
  ["right/top", ["a", "b", "c"], ["c"], "right"],
  ["right/twoApart", ["a", "b", "c", "d", "e"], ["a", "c"], "right"],
  ["left/block", ["a", "b", "c", "d"], ["c", "d"], "left"],
  ["right/group", ["a", "g1", "g2", "b"], ["g1", "g2"], "right"],
  ["right/withLabel", ["r", "t", "x"], ["r"], "right"],
  ["right/skipsDeleted", ["a", "del", "b"], ["a"], "right"],
];
```

generator 建立元件：`id`、`type: "rectangle"`，`g1`／`g2` 有 `groupIds: ["G"]`，`t` 是 `r` 的標籤（`type: "text"`、`containerId: "r"`，`r` 的 `boundElements` 指向它），`del` 的 `isDeleted: true`，其他欄位用 `index`（`a0`、`a1`…）、`version: 1`、`versionNonce: 0`、`updated: 1`；`appState` 的 `selectedElementIds` 由選取清單組成，`editingGroupId: null`。bundle 匯出 `moveOneLeft`、`moveOneRight`，輸出 `[{id, index, version}]`（跟既有 `fractional_index` 群組的摘要一樣）。

Run: `cd tools/baseline && npm run scene`
Expected: `zindex: 8 cases`，其他群組不變。

- [ ] **Step 2：Rust 基準測試（先失敗）並實作 `zindex.rs`**

基準測試建立同樣的元件（用 `sample` 的完整欄位版本，保證是型別化元件；`index` 照 case），呼叫 `move_one`，輸出同樣的摘要比較。實作照 JS port。

- [ ] **Step 3：`Editor` 指令**

`Command::BringForward`／`SendBackward`：idle 且有選取時呼叫 `move_one`，有結果就換掉元件並 `finish_edit`（一步 undo）。在 `editor_select.rs` 加測試：三個矩形選中間那個，`BringForward` 後順序是 `a c b`、`Undo` 回到 `a b c`、選取不變。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene tools/baseline
git commit -m "Move selected elements one layer up or down"
```

---

### Task 3：複製一份與 Excalidraw 剪貼簿

**Files:**
- Create: `crates/scene/src/duplicate.rs`、`crates/scene/src/clipboard.rs`、`crates/scene/tests/editor_clipboard.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/editor/mod.rs`、`crates/scene/tests/baseline.rs`、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`；新增 `crates/scene/tests/baseline/duplicate.json`

**Interfaces:**
- Consumes: M5 的 `new_element::{new_text_element, TextProps}`、`text::TextMeasure`；`fractional_index::sync_moved_indices`；`geometry::GeometryCache::common_bounds`。
- Produces:

```rust
// scene::duplicate
pub enum DuplicateMode {
    /// `type: "in-place"`: each duplicate right after its original, offset by `offset`.
    InPlace { offset: [f64; 2] },
    /// `type: "everything"`: every given element duplicated, appended in order.
    Everything,
}
pub struct Duplicated {
    /// The full element list with the duplicates inserted (in-place) or only the duplicates
    /// (everything), indices not yet synced.
    pub elements: Vec<Element>,
    /// Ids of the new elements.
    pub new_ids: Vec<String>,
}
/// `duplicateElements` with `randomizeSeed: true`: fresh ids and seeds, group ids remapped,
/// bindings and containers remapped by `fixDuplicatedBindingsAfterDuplication` (references to
/// elements that were not duplicated are dropped).
pub fn duplicate_elements(elements: &[Element], ids: &Selection, mode: DuplicateMode,
                          env: &mut impl Env) -> Duplicated;

// scene::clipboard
/// `serializeAsClipboardJSON` for the non-deleted selected elements plus their bound text:
/// `{"type":"excalidraw/clipboard","elements":[...],"files":{}}`.
pub fn serialize(file: &SceneFile, selection: &Selection) -> Option<String>;
pub enum Pasted {
    Elements(Vec<Element>),
    Text(String),
}
/// An `excalidraw/clipboard` payload's elements (also accepts a whole `excalidraw` file), or
/// otherwise the raw text. `None` for empty text.
pub fn parse(text: &str) -> Option<Pasted>;

// scene::editor
impl<E: Env> Editor<E> {
    /// The clipboard JSON for the selection; `None` when nothing is selected.
    pub fn copy_selection(&self) -> Option<String>;
    /// Pastes clipboard `text` centered on `at` (`addElementsFromPasteOrLibrary`), or as a new
    /// text element at `at` when it is not Excalidraw data; selects what was pasted. One step.
    pub fn paste(&mut self, text: &str, at: [f64; 2], measure: &mut dyn TextMeasure) -> bool;
}
// Command::Duplicate (Ctrl+D): InPlace with offset DEFAULT_GRID_SIZE / 2 on both axes
// (actionDuplicateSelection), selecting the duplicates.
```

要讀的 JS：`packages/element/src/duplicate.ts`（`duplicateElement`、`duplicateElements`、`_deepCopyElement`）；`packages/element/src/binding.ts` 的 `fixDuplicatedBindingsAfterDuplication`；`packages/excalidraw/actions/actionDuplicateSelection.tsx`；`packages/excalidraw/clipboard.ts` 的 `serializeAsClipboardJSON`、`parseClipboard`、`parseClipboardEventTextData`；`packages/excalidraw/components/App.tsx` 的 `pasteFromClipboard`、`addElementsFromPasteOrLibrary`、`addTextFromPaste`；`packages/common/src/constants.ts` 的 `DEFAULT_GRID_SIZE`。貼上的元件先照 `restoreElements` 做 M4a 已有的修復（`edit::repair_on_load` 的重複 id 與 index），並丟掉 `isDeleted` 的元件。

- [ ] **Step 1：JS 基準（`duplicateElements`）**

`cases.mjs` 加 `duplicateCases`：`[name, elements(用 sample 同樣的完整欄位建), selected ids, mode]`，至少：單一矩形 in-place；有標籤的容器（選容器，標籤一起複製並重新綁定）；箭頭綁在兩個矩形上、只選箭頭（綁定被清掉）與三個都選（綁定改指新 id）；群組；everything 模式。generator 用 `duplicateElements({type, elements, idsOfElementsToDuplicate, appState, randomizeSeed: true, overrides})`（in-place 的 `overrides` 照 `actionDuplicateSelection` 加 `DEFAULT_GRID_SIZE / 2`），再 `syncMovedIndices`，輸出用 M5 skeleton 的 `normalizeSkeletonOutput`（搬到共用的函數，`groupIds` 也照輸出順序改名成 `g0`、`g1`…），`runCase(..., { exactDespiteRandom: true })`。

Run: `cd tools/baseline && npm run scene`
Expected: `duplicate` 群組產生，其他群組不變（`skeleton.json` 也必須不變）。

- [ ] **Step 2：Rust 基準（先失敗）、實作 `duplicate.rs`**

Rust 測試用同樣的正規化（把 `baseline.rs` 既有的 `normalize_skeleton_output` 擴充為也改名 `groupIds`，skeleton 群組的結果不受影響）。

- [ ] **Step 3：`clipboard.rs` 與 `Editor` 方法的失敗測試**（`crates/scene/tests/editor_clipboard.rs`）

```rust
mod support;

use scene::clipboard::{self, Pasted};
use scene::editor::Command;
use scene::sample::{self, CharWidthMeasure};
use serde_json::{Value, json};
use support::*;

#[test]
fn copy_then_paste_round_trips_through_excalidraw_clipboard_json() {
    let mut e = editor(vec![
        sample::with(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
                     json!({"boundElements": [{"id": "t", "type": "text"}]})),
        sample::text("t", [30.0, 12.5, 40.0, 25.0], "hi", Some("r")),
    ]);
    e.command(Command::SelectAll);
    let text = e.copy_selection().expect("something selected");
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["type"], json!("excalidraw/clipboard"));
    assert_eq!(value["elements"].as_array().unwrap().len(), 2);

    assert!(e.paste(&text, [500.0, 500.0], &mut CharWidthMeasure));
    let live: Vec<Value> = e.file().elements.iter().filter(|x| !x.is_deleted()).map(|x| x.to_value()).collect();
    assert_eq!(live.len(), 4);
    let pasted_rect = &live[2];
    assert_eq!((pasted_rect["x"].clone(), pasted_rect["y"].clone()), (json!(450.0), json!(475.0)));
    assert_ne!(pasted_rect["id"], json!("r"));
    assert_eq!(live[3]["containerId"], pasted_rect["id"]);
    assert_eq!(e.selection().len(), 2);
    assert!(e.command(Command::Undo));
    assert_eq!(e.file().elements.iter().filter(|x| !x.is_deleted()).count(), 2);
}

#[test]
fn plain_text_pastes_as_a_text_element() {
    let mut e = editor(vec![]);
    assert!(matches!(clipboard::parse("hello"), Some(Pasted::Text(_))));
    assert!(e.paste("hello", [10.0, 20.0], &mut CharWidthMeasure));
    let v = e.file().elements[0].to_value();
    assert_eq!(v["type"], json!("text"));
    assert_eq!(v["text"], json!("hello"));
    assert!(clipboard::parse("").is_none());
}

#[test]
fn duplicate_offsets_by_half_a_grid_and_selects_the_copies() {
    let mut e = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0])]);
    e.command(Command::SelectAll);
    assert!(e.command(Command::Duplicate));
    let copy = e.file().elements[1].to_value();
    assert_eq!((copy["x"].clone(), copy["y"].clone()), (json!(10.0), json!(10.0)));
    assert!(e.selection().contains(copy["id"].as_str().unwrap()));
    assert!(!e.selection().contains("r"));
}
```

`paste` 的純文字位置照 `addTextFromPaste`（檢查 JS：它以游標為文字的左上角或中心，照 JS 決定期望值；上面的測試只檢查類型與文字）。

Run: `cargo test -p scene --test editor_clipboard`
Expected: 編譯失敗。

- [ ] **Step 4：實作 `clipboard.rs` 與 `Editor` 方法，全部檢查並 commit**

```bash
git add crates/scene tools/baseline
git commit -m "Duplicate, copy and paste elements in Excalidraw's clipboard format"
```

---

### Task 4：橡皮擦

**Files:**
- Create: `crates/scene/src/editor/erase.rs`、`crates/scene/tests/editor_erase.rs`
- Modify: `crates/scene/src/editor/mod.rs`

**Interfaces:**
- Produces:

```rust
impl<E: Env> Editor<E> {
    /// Ids of the elements the current eraser stroke will delete (drawn faded).
    pub fn pending_erasure(&self) -> &HashSet<String>;
}
```

`Tool::Eraser` 的手勢：`pointer_down` 開始一筆並把按下點碰到的元件加入待擦除；`pointer_move` 把上一點到目前點的線段交到的元件加入；`pointer_up` 把待擦除的元件照 `edit::delete_selection` 的規則刪除（一步 undo），清空集合；`Command::Escape` 在拖曳中放棄這一筆（清空、不刪除）。工具切換時同 `pointer_up`。

要讀的 JS：`packages/excalidraw/eraser/index.ts`（`EraserTrail` 的 `updateElementsToBeErased` 與它的命中判定：線段與元件外框相交、封閉填色形狀的點在內部、freedraw 用外框線段、群組整組、綁定文字跟容器）；`App.tsx` 的 `handleEraser`、pointer up 裡 `eraseElements`（12744 行附近）與 `ELEMENT_READY_TO_ERASE_OPACITY`。命中判定盡量用 M4a 的 `collision` 模組（`hit_element_itself`、`is_point_in_element`、`GeometryCache::linear_collision_shape`）；JS 用線段交集的地方，線段對線段的距離用閾值 `collision::hit_threshold(element, zoom)`。鎖定的元件不擦除（`locked`）。

- [ ] **Step 1：失敗的測試**（`crates/scene/tests/editor_erase.rs`）

```rust
mod support;

use scene::editor::{Command, Tool};
use scene::sample;
use serde_json::json;
use support::*;

#[test]
fn a_stroke_across_two_shapes_erases_both_in_one_step() {
    let mut e = editor(vec![
        sample::generic("rectangle", "a", [0.0, 0.0, 50.0, 50.0]),
        sample::generic("rectangle", "b", [100.0, 0.0, 50.0, 50.0]),
        sample::generic("rectangle", "keep", [0.0, 200.0, 50.0, 50.0]),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(-10.0, 25.0));
    e.pointer_move(at(60.0, 25.0));
    assert!(e.pending_erasure().contains("a"));
    e.pointer_move(at(160.0, 25.0));
    assert_eq!(e.pending_erasure().len(), 2);
    e.pointer_up(at(160.0, 25.0));
    assert!(e.pending_erasure().is_empty());
    let deleted: Vec<bool> = e.file().elements.iter().map(|x| x.is_deleted()).collect();
    assert_eq!(deleted, vec![true, true, false]);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.iter().all(|x| !x.is_deleted()));
}

#[test]
fn a_click_erases_the_element_under_it_with_its_label() {
    let mut e = editor(vec![
        sample::with(sample::with(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"backgroundColor": "#a5d8ff"})),
            json!({"boundElements": [{"id": "t", "type": "text"}]})),
        sample::text("t", [30.0, 12.5, 40.0, 25.0], "hi", Some("r")),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(50.0, 25.0));
    e.pointer_up(at(50.0, 25.0));
    assert!(e.file().elements.iter().all(|x| x.is_deleted()));
}

#[test]
fn escape_abandons_the_stroke_and_locked_elements_survive() {
    let mut e = editor(vec![
        sample::generic("rectangle", "a", [0.0, 0.0, 50.0, 50.0]),
        sample::with(sample::generic("rectangle", "l", [100.0, 0.0, 50.0, 50.0]), json!({"locked": true})),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(-10.0, 25.0));
    e.pointer_move(at(160.0, 25.0));
    assert!(!e.pending_erasure().contains("l"));
    assert!(e.command(Command::Escape));
    e.pointer_up(at(160.0, 25.0));
    assert!(e.file().elements.iter().all(|x| !x.is_deleted()));
}
```

- [ ] **Step 2：實作 `erase.rs`，全部檢查並 commit**

```bash
git add crates/scene
git commit -m "Add the eraser tool"
```

---

### Task 5：文字工具與文字編輯（scene 端）

**Files:**
- Create: `crates/scene/src/editor/text.rs`、`crates/scene/tests/editor_text.rs`
- Modify: `crates/scene/src/editor/mod.rs`、`crates/scene/src/editor/select.rs`

**Interfaces:**
- Consumes: M5 的 `new_text_element`、`batch::add::bind_label`（`pub(crate)`；需要時放寬到 crate 內可用即可）、`text::measure_text`、`transform::bound_text_position`、`fractional_index::sync_moved_indices`；Task 1 的 `ItemStyle::{font_family, font_size}`。
- Produces:

```rust
// scene::editor::text
#[derive(Clone, Debug, PartialEq)]
pub struct TextEditing {
    /// The text element being edited; `None` while typing a new one.
    pub element_id: Option<String>,
    /// The container a new label will be bound to.
    pub container_id: Option<String>,
    /// Current text shown in the editor.
    pub text: String,
    /// Top-left of the editor box in scene coordinates.
    pub origin: [f64; 2],
    /// Width the editor box starts at (the element's width, or 0 for new text).
    pub width: f64,
    pub font_family: f64,
    pub font_size: f64,
    pub line_height: f64,
    pub text_align: String,
    pub stroke_color: String,
    pub opacity: f64,
    pub angle: f64,
}

impl<E: Env> Editor<E> {
    pub fn text_editing(&self) -> Option<&TextEditing>;
    /// Writes the edited text back as one history step (`textWysiwyg`'s `onSubmit` and
    /// `handleTextWysiwyg`'s submit branch): a new, non-empty text becomes a text element (or
    /// the container's label); an existing text takes the new text, re-measured, and an empty
    /// one is deleted. Ends editing and returns the tool to Selection.
    pub fn commit_text(&mut self, text: &str, measure: &mut dyn TextMeasure) -> bool;
    /// Selection tool double click (`handleCanvasDoubleClick`): edits the text or container
    /// label under the pointer, starts a label on a container without one, or starts a new
    /// text at the pointer on empty canvas. Returns whether editing started.
    pub fn double_click(&mut self, event: PointerEvent) -> bool;
}
```

行為：

- `Tool::Text` 的 `pointer_down`（`handleTextOnPointerDown` → `startTextEditing`）：點到既有文字就編輯它；點到 rectangle／diamond／ellipse 就編輯或建立它的標籤；否則在按下的位置開始新文字（`textAlign: left`，左上角在按下點，照 JS 的 `y` 會減半行高，以 JS 為準）。字型、字級、顏色、透明度取 `ItemStyle`。
- 編輯中：`Command::Escape` 交給 app（app 先呼叫 `commit_text` 再處理）；`set_tool`、`finish_pending_gesture` 不會丟掉編輯中的文字（app 在切換前 commit）。`is_idle()` 在編輯中回 `false`，所以 AI 的修改會等編輯結束。
- `commit_text`：新文字用 `new_text_element`（`ElementProps` 取 `ItemStyle` 的線條色、透明度，其餘預設），加到尾端並 `sync_moved_indices`；標籤用 `bind_label`；既有文字改 `text`、`originalText`，`measure_text` 重算寬高，標籤重新定位，獨立文字左上角不動；空字串時新文字不建立，既有文字照 `delete_selection` 刪除（標籤刪除時容器的 `boundElements` 移除它）。之後選取該文字（或容器），工具回到 Selection。

要讀的 JS：`App.tsx` 的 `handleTextOnPointerDown`、`startTextEditing`、`handleTextWysiwyg`、`handleCanvasDoubleClick`、`getTextElementAtPosition`；`packages/excalidraw/wysiwyg/textWysiwyg.tsx` 的 `onSubmit`；`newElement.ts` 的 `newTextElement`。

- [ ] **Step 1：失敗的測試**（`crates/scene/tests/editor_text.rs`）

```rust
mod support;

use scene::editor::{Command, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;
use support::*;

#[test]
fn text_tool_creates_a_measured_text_element_in_one_step() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.pointer_down(at(100.0, 100.0));
    e.pointer_up(at(100.0, 100.0));
    let editing = e.text_editing().expect("editing").clone();
    assert_eq!(editing.element_id, None);
    assert_eq!(editing.font_size, 20.0);
    assert!(!e.is_idle());
    assert!(e.commit_text("hi\n中文", &mut CharWidthMeasure));
    assert!(e.text_editing().is_none());
    assert_eq!(e.tool(), Tool::Selection);
    let v = e.file().elements[0].to_value();
    assert_eq!(v["text"], json!("hi\n中文"));
    assert_eq!(v["height"], json!(50.0));
    assert!(e.selection().contains(v["id"].as_str().unwrap()));
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
}

#[test]
fn committing_empty_new_text_creates_nothing() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.pointer_down(at(0.0, 0.0));
    e.pointer_up(at(0.0, 0.0));
    assert!(!e.commit_text("", &mut CharWidthMeasure));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));
}

#[test]
fn double_click_edits_existing_text_and_empty_deletes_it() {
    let mut e = editor(vec![sample::text("t", [0.0, 0.0, 36.0, 25.0], "old", None)]);
    assert!(e.double_click(at(10.0, 10.0)));
    assert_eq!(e.text_editing().unwrap().element_id.as_deref(), Some("t"));
    assert_eq!(e.text_editing().unwrap().text, "old");
    assert!(e.commit_text("longer", &mut CharWidthMeasure));
    assert_eq!(e.file().elements[0].to_value()["width"], json!(6.0 * 12.0));
    assert!(e.double_click(at(10.0, 10.0)));
    assert!(e.commit_text("", &mut CharWidthMeasure));
    assert!(e.file().elements[0].is_deleted());
}

#[test]
fn double_click_on_a_container_adds_a_centered_label() {
    let mut e = editor(vec![sample::generic("rectangle", "r", [0.0, 0.0, 200.0, 100.0])]);
    assert!(e.double_click(at(100.0, 50.0)));
    assert_eq!(e.text_editing().unwrap().container_id.as_deref(), Some("r"));
    assert!(e.commit_text("box", &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["containerId"], json!("r"));
    assert_eq!(label["x"], json!(5.0 + (190.0 / 2.0 - 36.0 / 2.0)));
    assert!(e.file().elements[0].bound_elements().contains(&(label["id"].as_str().unwrap(), "text")));
}

#[test]
fn double_click_on_empty_canvas_starts_new_text_there() {
    let mut e = editor(vec![]);
    assert!(e.double_click(at(40.0, 40.0)));
    assert_eq!(e.text_editing().unwrap().element_id, None);
    assert_eq!(e.text_editing().unwrap().container_id, None);
}
```

（`double_click_on_empty_canvas` 的 `origin` 與 `text_tool_creates` 的位置值照 JS 的 `startTextEditing` 決定，測試不綁死。）

Run: `cargo test -p scene --test editor_text`
Expected: 編譯失敗。

- [ ] **Step 2：實作 `text.rs`，全部檢查並 commit**

```bash
git add crates/scene
git commit -m "Add the text tool and text editing to the editor"
```

---

### Task 6：渲染的 `faded` 與 `hidden`、egui 字型

**Files:**
- Create: `crates/app/src/fonts.rs`
- Modify: `crates/app/src/render/gpu.rs`、`crates/app/src/render/plan.rs`、`crates/app/src/render/offscreen.rs`（`CanvasFrame` 新欄位）、`crates/app/src/lib.rs`、`crates/app/tests/support/mod.rs`、`crates/app/tests/gpu_shapes.rs`（新測試）

**Interfaces:**
- Produces:

```rust
// render::gpu::CanvasFrame 新增
/// Elements drawn at `ERASE_PENDING_ALPHA` of their opacity (the eraser's pending set).
pub faded: Arc<HashSet<String>>,
/// Elements not drawn at all (the text being edited).
pub hidden: Arc<HashSet<String>>,
// render::plan
/// `ELEMENT_READY_TO_ERASE_OPACITY` (20) as a fraction.
pub const ERASE_PENDING_ALPHA: f32 = 0.2;
// View 新增 faded、hidden（借用 CanvasFrame 的）

// app::fonts
/// Registers napkin-hand, napkin-sans and napkin-code as egui font families (named after
/// themselves), each falling back to the system CJK font when one is found, and adds that CJK
/// font as a fallback of egui's proportional family, so IME candidates and typed Chinese show.
pub fn install(ctx: &egui::Context);
/// `render::text::bundled_family` as an egui family.
pub fn egui_family(font_family: f64) -> egui::FontFamily;
```

- [ ] **Step 1：失敗的 GPU 測試**（`gpu_shapes.rs`）：兩個實心藍矩形，`faded` 含第一個、`hidden` 含第二個；讀回後第一個中心像素的藍色明顯淡（和背景混合，與完全不透明的值差距大於 100），第二個中心是背景白。`support` 的 `render*` 函數加參數或新增一個接受 `faded`／`hidden` 的變體；既有測試傳空集合。

- [ ] **Step 2：實作**：`plan_frame` 跳過 `hidden` 裡的元件（連同它是標籤時仍照常畫容器），`element_alpha` 對 `faded` 裡的元件乘以 `ERASE_PENDING_ALPHA`（容器在 faded 時它的標籤也變淡，照 Excalidraw `renderElement` 對 bound text 的處理）。所有建 `CanvasFrame` 的地方（`napkin_app.rs`、`offscreen::GpuRasterizer`、測試）補上空集合。

- [ ] **Step 3：`fonts.rs`**：`egui::FontDefinitions::default()`，加入三個 `include_bytes!` 字型（與 `render::text::font_system` 同樣的檔案），CJK 字型用 `glyphon::fontdb::Database::load_system_fonts` 後查詢家族名（`Noto Sans CJK TC`、`SC`、`JP` 依序），取出資料與 face index（`.ttc`，`egui::FontData::from_owned(..).index`）。單元測試：`egui_family(5.0)` 是 `Name("napkin-hand")`、`egui_family(42.0)` 也是；`install` 對一個新的 `egui::Context` 呼叫後 `ctx.fonts(..)` 能取得三個家族（用 `egui::Context::default()`，跑一次 `ctx.run(Default::default(), |_| {})` 讓字型生效）。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Fade pending erasures, hide the text being edited, register napkin fonts in egui"
```

---

### Task 7：工具列與新的快捷鍵、剪貼簿接線

**Files:**
- Create: `crates/app/src/toolbar.rs`
- Modify: `crates/app/src/edit_input.rs`、`crates/app/src/napkin_app.rs`、`crates/app/src/lib.rs`

**Interfaces:**
- Produces:

```rust
// app::toolbar
/// The toolbar's tools in order, with their shortcut hint (Tools.tsx): Hand, Selection,
/// Rectangle, Diamond, Ellipse, Arrow, Line, Freedraw, Text, Eraser.
pub const TOOLS: [(Tool, &str); 10];
/// Draws the toolbar at the top centre; returns the tool the user clicked.
pub fn show(ctx: &egui::Context, current: Tool, colors: ToolbarColors) -> Option<Tool>;

// app::edit_input::EditorInput 新增
Copy,
Paste(String),
// key_input：T/Num8 -> Tool::Text，E/Num0 -> Tool::Eraser，Ctrl+D -> Command::Duplicate，
// Ctrl+] -> BringForward，Ctrl+[ -> SendBackward（egui 0.36 的 Key::CloseBracket/OpenBracket），
// Ctrl+C -> Copy；egui::Event::Paste(text) -> Paste(text)（Ctrl+V 由 egui 轉成 Paste 事件）。
// FrameInput 新增 pointer_over_ui: bool；為真時不送 Down。
// egui::Event::PointerButton 的雙擊：用 egui 的 `PointerState::button_double_clicked`
// 或自行以時間與距離判斷，送 EditorInput::DoubleClick(PointerEvent)。
DoubleClick(PointerEvent),
```

`napkin_app.rs`：`EditorInput::Copy` → `editor.copy_selection()` 有值時 `ctx.copy_text`；`Paste(text)` → `editor.paste(text, 游標的場景座標或畫面中心, measure)`（`FontMeasure` 沿用 M5 的 lazily 建立）；`DoubleClick` → `editor.double_click`。原本的 `tool_label` 小字換成工具列；畫布名稱仍在右上。

- [ ] **Step 1：`edit_input` 的失敗測試**：新快捷鍵對應（每個鍵一個 case，含 `Ctrl+]`、`Ctrl+[`、`T`、`8`、`E`、`0`、`Ctrl+D`、`Ctrl+C`）、`Event::Paste` 轉成 `Paste`、`pointer_over_ui` 為真時按下不產生 `Down`、雙擊產生 `DoubleClick`。

- [ ] **Step 2：`toolbar.rs`**：`egui::Area` 固定在上方中間，一排按鈕，每個按鈕用 painter 畫圖示（決定 8），目前工具高亮（主題的 `accent`），hover 時 tooltip 顯示名稱與快捷鍵。單元測試：`TOOLS` 的順序與快捷鍵字串（`"H"`、`"V 1"`、`"R 2"`、`"D 3"`、`"O 4"`、`"A 5"`、`"L 6"`、`"P 7"`、`"T 8"`、`"E 0"`）。

- [ ] **Step 3：接線與煙霧測試**：照 Global Constraints 的方式開視窗截圖，確認工具列在上方中間、選取工具高亮；關閉。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Add the toolbar, new shortcuts and clipboard wiring"
```

---

### Task 8：屬性面板

**Files:**
- Create: `crates/app/src/properties_panel.rs`
- Modify: `crates/app/src/napkin_app.rs`、`crates/app/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 的 `Editor::{panel, set_property}`、`PanelState`、`Section`、`Property`。
- Produces:

```rust
/// Excalidraw's quick picks (`DEFAULT_ELEMENT_STROKE_PICKS`, `DEFAULT_ELEMENT_BACKGROUND_PICKS`)
/// and the full palette grid (`DEFAULT_ELEMENT_STROKE_COLOR_PALETTE` /
/// `DEFAULT_ELEMENT_BACKGROUND_COLOR_PALETTE`, shade index 3 for strokes and 1 for
/// backgrounds, as `getColorPalette` picks them).
pub const STROKE_PICKS: [&str; 5];
pub const BACKGROUND_PICKS: [&str; 5];
/// Draws the panel on the left when `state.sections` is non-empty; returns the property the
/// user picked.
pub fn show(ctx: &egui::Context, state: &PanelState, colors: PanelColors, dark: bool) -> Option<Property>;
```

要讀的 JS：`packages/common/src/colors.ts`（`COLOR_PALETTE`、`DEFAULT_ELEMENT_STROKE_PICKS`、`DEFAULT_ELEMENT_BACKGROUND_PICKS`、兩個 `_COLOR_PALETTE`、`DEFAULT_ELEMENT_STROKE_COLOR_INDEX`、`DEFAULT_ELEMENT_BACKGROUND_COLOR_INDEX`）；`actionProperties.tsx` 各 `PanelComponent` 的選項與順序（填充樣式 hachure／cross-hatch／solid，線寬 thin／bold／extraBold 對應 1／2／4，線條樣式，潦草度 architect／artist／cartoonist，邊角 sharp／round，箭頭類型 sharp／round，箭頭頭部清單，字型 Excalifont／Nunito／Comic Shanns = 5／6／8，字級 S／M／L／XL = 16／20／28／36，透明度 0–100 slider）。

面板內容：每個 section 一列，標題用小字；顏色是快速色塊加一個「更多」按鈕展開調色盤格子；`None`（混合）時不高亮任何選項。深色主題時色塊照 `scene::color::apply_dark_mode_filter` 顯示（檔案存的仍是淺色）。

- [ ] **Step 1：單元測試**：`STROKE_PICKS == ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"]`、`BACKGROUND_PICKS == ["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99"]`（照 JS 驗證，不一致時照 JS 改測試）；調色盤格子的色值數量與 JS 一致。

- [ ] **Step 2：實作與接線**：`napkin_app.rs` 每幀取 `editor.panel()`，`show` 回傳的 `Property` 交給 `editor.set_property(p, measure)`。

- [ ] **Step 3：煙霧測試**：開一個含矩形的暫存檔（`HOME` 暫時目錄、檔案裡該矩形已被選取不會發生，所以只截空白畫面；面板在建立工具時才顯示，這一步只確認沒有崩潰、工具列仍在），關閉。面板的互動交給人工驗收。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Add the property panel"
```

---

### Task 9：文字編輯的 egui 疊層

**Files:**
- Create: `crates/app/src/text_edit.rs`
- Modify: `crates/app/src/napkin_app.rs`、`crates/app/src/lib.rs`

**Interfaces:**
- Consumes: Task 5 的 `Editor::{text_editing, commit_text}`、Task 6 的 `fonts::egui_family`、`CanvasFrame::hidden`。
- Produces:

```rust
/// What the overlay decided this frame.
pub enum TextEditOutcome { Editing, Commit(String) }
/// Shows a multiline `TextEdit` over `editing`'s box: font from `fonts::egui_family`, size
/// `font_size * zoom`, colour `stroke_color` (dark-mode filtered when `dark`), no frame, width
/// growing with the text. Focuses it on the first frame. Escape, Ctrl+Enter or losing focus
/// commits; Enter inserts a newline (textWysiwyg).
pub fn show(ui: &mut egui::Ui, editing: &TextEditing, buffer: &mut String, camera: Camera,
            canvas_origin: egui::Pos2, dark: bool, first_frame: bool) -> TextEditOutcome;
```

`napkin_app.rs`：`editor.text_editing()` 有值時保存一個 `String` 緩衝（開始時複製 `editing.text`），把正在編輯的元件 id（有的話）與它是標籤時的 id 放進 `hidden`；`Commit(text)` 時 `editor.commit_text(&text, measure)`。編輯中：畫布上的 `Down` 事件先 commit 再照常處理；工具切換、`Esc` 先 commit；`edit_input` 在 egui 有鍵盤焦點（`keyboard_taken`）時本來就不送快捷鍵，所以輸入 `r`、`t` 等字不會切工具。旋轉過的容器照舊顯示不旋轉的編輯框（egui 不能旋轉 `TextEdit`），commit 後才以元件的角度畫出。

- [ ] **Step 1：單元測試**：`text_edit` 內把「編輯框的螢幕位置與字級」寫成純函數 `layout(editing, camera, canvas_origin) -> (egui::Pos2, f32)` 並測試縮放 2 倍時位置與字級加倍；`Esc`／`Ctrl+Enter` 判斷寫成純函數測試。

- [ ] **Step 2：實作與接線**。

- [ ] **Step 3：煙霧測試**：只開關視窗確認沒有崩潰（文字編輯的互動與中文輸入交給人工驗收）。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Edit text in an egui overlay on the canvas"
```

---

### Task 10：skill 與人工驗收清單

**Files:**
- Modify: `skills/napkin/SKILL.md`（只在 M4b 改變了 Claude 需要知道的行為時；例如 `napkin scene` 的格式沒變就不動）

- [ ] **Step 1：確認 AI 介面仍然正確**：`cargo test -p app --test skill_examples` 與 `--test control_socket` 通過；`Editor::is_idle()` 在文字編輯中為 false，所以 AI 的 `apply` 會等使用者打完字，skill 的「忙碌時重試」說明仍然成立，不需要改。若有改動就 commit：

```bash
git add skills/napkin/SKILL.md
git commit -m "Update the napkin skill for text editing"
```

- [ ] **Step 2：交給使用者的人工驗收**（主 spec §9.3 的 #2、#4 與 M4b 功能）：
  1. 工具列點選每個工具、快捷鍵 `T` `8` `E` `0` 切換正確。
  2. 按 `T` 在畫布上點一下，用 fcitx5 輸入中文：候選字窗位置正確、preedit 顯示正常，`Esc` 結束後文字留在畫布上，`Ctrl+Z` 一次復原。
  3. 雙擊文字修改、雙擊矩形加標籤。
  4. 選取元件後用屬性面板改顏色、填充、線寬、字級，`Ctrl+Z` 每次復原一步。
  5. `Ctrl+C` 後到 excalidraw.com `Ctrl+V` 貼上，反向從 excalidraw.com 複製貼進 napkin。`Ctrl+D` 複製一份。
  6. `Ctrl+]`／`Ctrl+[` 改變重疊順序。
  7. 橡皮擦劃過幾個元件：劃過時變淡，放開後刪除，`Ctrl+Z` 復原。
  8. 在 excalidraw.com 存一份含圖片的檔案 → 在 napkin 裡改一個字並存檔 → 丟回 excalidraw.com，圖片仍在（主 spec §9.3 #4）。

---

## 自我檢查

- 主 spec §1.1 的編輯項目：圖層上下移（Task 2）、複製／貼上／複製一份（Task 3）、橡皮擦（Task 4）、文字（Task 5、9）。綁定跟隨與換行不在 M4b（與 spec 不同的地方 1）。
- §7.1 快捷鍵：Task 7。§7.2 版面：工具列 Task 7、屬性面板 Task 1、8。§7.3 文字編輯：Task 5、6、9。§7.5 `Esc` 的順序：文字編輯（Task 9）→ 正在畫的形狀（M4a）→ 取消選取（M4a）；橡皮擦的放棄在 Task 4。
- §5.7 一步 undo：每個 scene 任務都有 undo 測試。§6.4 提示畫在上層：待擦除變淡用渲染的 alpha（Task 6），不進快取。
- §9.3 #2、#4：Task 10 的人工驗收。
