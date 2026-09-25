# napkin M4a：編輯核心 Implementation Plan

> Historical record, frozen 2026-09-17. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 napkin 裡建立、選取、搬移、縮放、刪除矩形、菱形、橢圓、線、箭頭與手繪筆畫，可以 undo／redo，修改在停頓 500 ms 後、失去焦點時與結束前寫回 `.excalidraw`，外部修改在取得焦點時重新載入。

**Architecture:** `scene` 新增純資料的編輯層：幾何與外框、點選判定、選取規則、控制點與縮放、場景修改、undo 歷史，以及互動狀態機 `scene::editor::Editor`。`Editor` 持有 `Arc<SceneFile>`，吃場景座標的抽象指標事件與指令，直接修改場景，並輸出選取框、控制點等覆蓋層資料；它不依賴 egui。`app` 把 egui 事件翻譯成 `Editor` 的輸入、用 egui painter 畫覆蓋層、把 `Arc<SceneFile>` 交給 M3 的渲染管線，並在背景執行緒原子寫檔。

**Tech Stack:** 沿用 M3：Rust 1.98.1（edition 2024）、eframe／egui／egui-wgpu 0.36.2、wgpu 30.0.1、glyphon 0.12.0、lyon 1.0.19、serde_json 1。M4a 不新增任何依賴。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（§1.1、§1.2、§4.2、§5.2 到 §5.7、§5.8 的刪除與搬移部分、§6.1、§6.4、§6.7、§7.1、§7.5、§8、§9.2）
**Roadmap:** `docs/decisions/plans/2026-09-13-napkin-roadmap.md`
**前置：** M3 已合併進 `master`（PR #2）。從合併後的 `master` 開分支 `m4a-editing-core`。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號或計畫。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 互動規則以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`（不存在時執行 `cd tools/baseline && npm ci && npm run scene`）。每個任務列出要讀的 JS 函數；port 時看 JS 原始碼，不看這份計畫的摘要。M4a 不產生 JS 基準，行為由 Rust 的單元測試與模擬事件測試鎖住。
- `scene` 與 `rough` 不能依賴 egui、wgpu 或任何繪圖、視窗 crate。workspace 的依賴清單不變。
- JS 數值語意一律走 `rough::js`（`math_round`、`atan2`、`hypot` 等）；亂數與時間一律走 `scene::env::Env`。
- `Element::Raw` 只允許改 `x`、`y`、`isDeleted`、`frameId`（改成 `null`）、`boundElements`（移除項目），以及 `bump_version` 寫的 `version`、`versionNonce`、`updated`（spec §5.2），而且只透過 Task 1 的 `Element` 方法。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫（Excalidraw `mutateElement` 的 `didChange`）。
- 無法解析的檔案絕不寫入（spec §8）。`--bench` 模式不寫任何檔案，包括 `last`。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試需要 wgpu adapter，沒有 adapter 時直接失敗，不跳過。

## 與 roadmap、spec 不同的地方

1. **M4 拆成 M4a 與 M4b**（與使用者確認）。M4a 是這份計畫。M4b：工具列與屬性面板、文字工具與文字編輯（人工驗收 #2、#4）、元件複製貼上與複製一份、圖層上下移、橡皮擦。
2. **undo 還原完全相同的元件**，包括 `version`、`versionNonce`、`updated`。Excalidraw 的 history 在 undo／redo 時遞增版本（`history.ts` 的 `excludedProperties`），為的是協作同步，napkin 沒有協作；完全還原讓 spec §9.2「全部 undo 後回到初始狀態」可以逐欄位比對。代價是同一個 `version` 可能先後對應兩種內容，所以渲染快取的鍵加上 `versionNonce`（Task 9）。
3. **外部修改比對 mtime 是否不同**，不只是「比較新」（spec §5.6）。從備份還原的舊檔 mtime 較舊，也應該重新載入。
4. **多點線段按 `Esc` 完成而不是放棄**，照 Excalidraw `actionFinalize`：保留已經點下的點，丟掉跟著游標的那一點。拖曳中的形狀按 `Esc` 照 spec §7.5 放棄。`Esc` 在沒有東西可取消時回傳「未處理」，收起視窗留給 M6。
5. **文字元件只能用角落與上下邊縮放**（字級等比例變化，`resizeSingleTextElement` 的 `n`／`s` 分支）。左右邊縮放要重新換行，M5 port 換行後才加。
6. **容器縮放時，綁定的文字只重新定位，不重新換行**（`computeBoundTextPosition`，文字寬高不變）。Excalidraw 的 `handleBindTextResize` 會重新換行並讓容器長高，留給 M5。
7. **搬移箭頭時，箭頭標籤存的座標跟著平移**（與使用者確認）。Excalidraw 拖動時不改標籤座標，畫的時候才從箭頭路徑算位置（`dragElements.ts` 的「skip arrow labels」）；napkin 的渲染用存的座標（M3 決定 2），不平移的話標籤會留在原地。M5 port `getBoundTextElementPosition` 後改回 Excalidraw 的做法。箭頭被縮放或拖動端點時，標籤不動。
8. **被綁定的元件移動時，箭頭端點不跟隨**（M5）。刪除時照 `fixBindingsAfterDeletion` 清掉兩端的紀錄（與使用者確認）。
9. **切換工具時完成進行中的多點線段**。Excalidraw 的 `setActiveTool` 把它原樣留在場景裡、不進 history；napkin 當成按 `Enter`。
10. **有旋轉過元件的多選不顯示控制點**。spec §1.2 規定旋轉過的元件不顯示縮放控制點；多選時 Excalidraw 會對旋轉元件強制等比例縮放，napkin 一併不提供。
11. **不做**：`Ctrl`+點選進入群組、`Alt`+拖曳複製、工具鎖定（`Q`）、方向鍵微移、格線與吸附、hover 高亮。spec 沒有列，M4b 以後再評估。

## 寫計畫時做的決定

1. **編輯層放在 `scene`**（spec §4.2）。`Editor` 用場景座標工作，每個指標事件帶 `zoom`，因為判定距離是 CSS 像素除以 `zoom`。
2. **一個手勢一步 undo**：指標按下時記住 `Arc<SceneFile>` 與選取，放開時比對前後差異記一筆（Excalidraw 在 pointer up 呼叫 `store.scheduleCapture()`）。指令（刪除）立即記一筆。只改選取不記。每筆紀錄帶選取的前後狀態，undo 時一併還原。最多 200 筆（spec §5.7）。
3. **歷史紀錄用位置比對**：同位置的元件前後不相等就記下；長度不同的部分是尾端的新增或移除。M4a 的操作不會改變元件順序（新元件一律加在尾端）；M4b 的圖層操作要擴充 `History`。
4. **載入時的修復**照 Excalidraw `restoreElements` 無條件做的兩件事：重複的 id 換成新的 nanoid、`syncInvalidIndices`。只在記憶體裡改，下次存檔才寫出。`repairBindings` 那一組（懸空的 `frameId`、`containerId`、`boundElements`、binding）留給 M5。
5. **幾何快取**：箭頭與線的外框要跑 rough.js 產生曲線，1000 個元件每次指標移動都重算太慢。`GeometryCache` 依 `(id, version, versionNonce)` 快取每個元件的外框與碰撞形狀，每個 id 只保留最新一份。
6. **`Arc<SceneFile>` 在編輯時用 `Arc::make_mut`**。渲染管線拿到的 `Arc` 在該幀畫完就釋放，所以下一次修改時引用數是 1，不會複製整個場景；每個手勢開頭為了 undo 保留的快照會讓第一次修改複製一次。`Editor::scene_clones` 計數，影格時間面板與 `--bench` 顯示它。
7. **新元件的預設樣式**照 `getDefaultAppState`，但圓角預設 `"round"`（Excalidraw 在測試環境用 `"sharp"`）。`ItemStyle` 是 M4b 屬性面板要改的對象。
8. **儲存位置**照 spec §5.4：畫布在 `~/Documents/napkin/`，`~/.local/state/napkin/last` 存上次開啟檔案的絕對路徑。有命令列參數時開那個檔案並記到 `last`；檔案不存在時從空白場景開始，第一次存檔時建立。沒有參數時開 `last`；`last` 不存在或指向不存在的檔案時開 `scratch`（不存在就立即建立）；`last` 指向的檔案無法解析時改開 `scratch` 並顯示通知（spec §8）。
9. **無法解析的檔案**（命令列指定的，或 `scratch` 本身）顯示錯誤、畫布唯讀、不自動存檔。外部修改後重新載入失敗也一樣。
10. **自動存檔在背景執行緒寫檔**：序列化加 `fsync` 在 1000 個元件的檔案上是數毫秒到數十毫秒，不能卡在 UI 執行緒。存檔工作拿 `Arc<SceneFile>` 的複本，視角另外傳，序列化時才寫進 `appState.napkin`，不修改記憶體裡的場景。結束時在 UI 執行緒同步寫。
11. **什麼時候寫檔**：元件有變動、停頓 500 ms、而且沒有手勢進行中時寫（手勢中停頓不寫）；失去焦點或結束時，元件或視角有變動就寫。只平移縮放不觸發 500 ms 的存檔。寫入失敗時保留在記憶體，右上角顯示紅色錯誤，每 5 秒重試，成功後消失（spec §8）。
12. **原子寫入**：同目錄暫存檔 `.<檔名>.napkin-tmp` → `write_all` → `sync_all` → `rename` → 對目錄 `sync_all`。寫入後記下檔案的 mtime，給外部修改比對用。
13. **重新載入**清空 undo 歷史與選取（spec §5.6），相機不動，渲染快取整個清空（`CanvasFrame::generation`）。上次存檔失敗、記憶體裡有未寫出的修改時不重新載入，錯誤訊息保持顯示。
14. **覆蓋層顏色**：選取框、控制點、框選用主題的 `accent`，框選填色用 `selection`，控制點內部填 `background`。
15. **效能驗收加拖動**：`--bench` 在平移、縮放之後拖動一個元件 5 秒。spec §6.1 只要求被拖動的元件重新產生網格；拖動大量元件時每幀重新三角化，不在這次驗收範圍。
16. **undo 的 property test 自己寫亂數**（固定種子的 LCG），不引入 proptest。
17. **`viewer.rs` 改名為 `napkin_app.rs`**（`Viewer` → `NapkinApp`），因為它從 M4a 起可以編輯；`document.rs` 由 `storage.rs` 取代。`app::sample` 搬到 `scene::sample`，因為 `scene` 的測試也需要完整欄位的元件 JSON。

## 檔案結構

```
crates/scene/src/
  sample.rs            完整欄位的元件 JSON（從 app 搬來）
  element.rs           Task 1 新增的讀寫方法
  geometry.rs          外框、旋轉、快取（getElementAbsoluteCoords、getElementBounds）
  collision.rs         點選判定（collision.ts、distance.ts）
  selection.rs         選取集合、群組、框選、全選、點中哪個元件
  transform.rs         控制點、縮放、綁定文字定位
  edit.rs              拖動、刪除、拖點、加入新元件、載入修復
  history.rs           undo／redo
  fractional_index.rs  midpoint 改成迴圈、sync_invalid_indices 失敗時重排
  file.rs              elements／appState 為 null、寫出時帶入視角
  editor/mod.rs        Editor：公開型別、事件分派、指令、覆蓋層
  editor/select.rs     選取工具的手勢
  editor/create.rs     建立工具的手勢
  editor/style.rs      ItemStyle
crates/scene/tests/
  support/mod.rs       測試用 Env 與事件輔助函數
  editor_select.rs     選取工具的互動測試
  editor_create.rs     建立工具的互動測試
  undo_property.rs     任意操作後全部 undo 回到初始狀態
crates/app/src/
  storage.rs           儲存位置、開檔、原子寫入、mtime
  autosave.rs          何時寫檔（純邏輯）
  writer.rs            背景寫檔執行緒
  edit_input.rs        egui 事件 → Editor 輸入
  overlay.rs           Overlay → egui 圖形
  napkin_app.rs        eframe::App（原 viewer.rs）
  bench.rs             加上拖動階段
  render/cache.rs      快取鍵加 versionNonce
  render/gpu.rs        CanvasFrame::generation
```

---

### Task 1：`scene::sample`、元件讀寫方法與幾何

**Files:**
- Create: `crates/scene/src/geometry.rs`
- Move: `crates/app/src/sample.rs` → `crates/scene/src/sample.rs`（`git mv`）
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/element.rs`、`crates/app/src/lib.rs`，以及所有 `crate::sample`／`app::sample` 的使用處（`crates/app/src/fixture.rs`、`render/cache.rs`、`render/plan.rs`、`render/tessellate.rs`、`render/text.rs`、`crates/app/tests/gpu_shapes.rs`、`crates/app/tests/gpu_text.rs`）

**Interfaces:**
- Consumes: M2 的 `Element`、`generate_element_shape`、`ShapeContext`；rough 的 `Drawable`、`Op`。
- Produces:

```rust
// element.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinearEnd { Start, End }

impl Element {
    /// `versionNonce`; a `Raw` element without a numeric one reads as 0.
    pub fn version_nonce(&self) -> f64;
    /// `locked === true`.
    pub fn is_locked(&self) -> bool;
    /// `groupIds`, innermost first; missing or non-string entries are skipped.
    pub fn group_ids(&self) -> Vec<&str>;
    /// A text element's `containerId` when it is a string; `None` for every other type.
    pub fn container_id(&self) -> Option<&str>;
    /// `boundElements` as `(id, type)` pairs; `null`, missing or malformed entries are skipped.
    pub fn bound_elements(&self) -> Vec<(&str, &str)>;
    /// `startBinding.elementId` / `endBinding.elementId` of a line or arrow.
    pub fn binding_target(&self, end: LinearEnd) -> Option<&str>;
    /// Sets `x` and `y` (a `Raw` element only when both are already numbers).
    pub fn set_position(&mut self, x: f64, y: f64);
    pub fn set_deleted(&mut self, deleted: bool);
    /// Removes every `boundElements` entry whose `id` is `id`; the array stays, possibly empty.
    pub fn remove_bound_element(&mut self, id: &str);
    /// Sets `startBinding` or `endBinding` to `null`.
    pub fn clear_binding(&mut self, end: LinearEnd);
    /// Sets a text element's `containerId` to `null`.
    pub fn clear_container_id(&mut self);
    /// Sets `frameId` to `null`.
    pub fn clear_frame_id(&mut self);
}

// geometry.rs
/// `[min_x, min_y, max_x, max_y]` in scene coordinates.
pub type Bounds = [f64; 4];

/// `pointRotateRads`.
pub fn rotate_point(point: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2];
/// `getSizeFromPoints`: `[width, height]` of the points' bounding box, `[0, 0]` for none.
pub fn size_from_points(points: &[[f64; 2]]) -> [f64; 2];
/// `getCubicBezierCurveBound`.
pub fn cubic_bezier_bounds(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]) -> Bounds;
/// `outer` contains `inner`, edges inclusive (`boundsContainBounds`).
pub fn bounds_contain(outer: &Bounds, inner: &Bounds) -> bool;
/// `getElementAbsoluteCoords` without bound text: the unrotated box and its center.
pub fn element_absolute_coords(element: &Element) -> Option<(Bounds, [f64; 2])>;
/// `getElementBounds`: the axis-aligned bounds of the rotated element.
pub fn element_bounds(element: &Element) -> Option<Bounds>;

/// Per-element geometry keyed by `(id, version, versionNonce)`, one entry per id.
#[derive(Default)]
pub struct GeometryCache { /* HashMap<String, Entry> */ }
impl GeometryCache {
    pub fn absolute_coords(&mut self, element: &Element) -> Option<(Bounds, [f64; 2])>;
    pub fn bounds(&mut self, element: &Element) -> Option<Bounds>;
    /// `getCommonBounds` over non-deleted elements with bounds.
    pub fn common_bounds<'a>(&mut self, elements: impl IntoIterator<Item = &'a Element>) -> Option<Bounds>;
    pub fn clear(&mut self);
}
```

- [ ] **Step 1：搬 `sample.rs`**

`git mv crates/app/src/sample.rs crates/scene/src/sample.rs`。模組 doc comment 改成「Complete element JSON for tests and fixtures」；`scene::SceneFile`、`scene::Element` 改成 `crate::SceneFile`、`crate::Element`；`scene/src/lib.rs` 加 `pub mod sample;`，`app/src/lib.rs` 拿掉 `pub mod sample;`；`crate::sample`、`app::sample` 全部改成 `scene::sample`。

Run: `cargo test --workspace`
Expected: 全部通過（搬移不改行為）。

- [ ] **Step 2：`element.rs` 的測試**

加在 `element.rs` 既有的 `tests` 模組：

```rust
    #[test]
    fn reads_editing_attributes_from_typed_and_raw_elements() {
        let mut value = rectangle();
        value["locked"] = json!(true);
        value["groupIds"] = json!(["inner", "outer"]);
        value["boundElements"] = json!([{"id": "t1", "type": "text"}, {"id": "a1", "type": "arrow"}]);
        let element = Element::from_value(value);
        assert!(matches!(element, Element::Rectangle(_)));
        assert!(element.is_locked());
        assert_eq!(element.group_ids(), vec!["inner", "outer"]);
        assert_eq!(element.bound_elements(), vec![("t1", "text"), ("a1", "arrow")]);
        assert_eq!(element.version_nonce(), 4.0);
        assert_eq!(element.container_id(), None);

        let image = Element::from_value(json!({
            "id": "i", "type": "image", "x": 1, "y": 2, "groupIds": ["g"], "locked": false,
            "boundElements": [{"id": "a", "type": "arrow"}], "versionNonce": 7
        }));
        assert!(!image.is_locked());
        assert_eq!(image.group_ids(), vec!["g"]);
        assert_eq!(image.bound_elements(), vec![("a", "arrow")]);
        assert_eq!(image.version_nonce(), 7.0);
    }

    #[test]
    fn reads_bindings_and_containers() {
        let arrow = Element::from_value(crate::sample::with(
            crate::sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]),
            json!({"startBinding": {"elementId": "r", "fixedPoint": [0.5, 0.5], "mode": "orbit"}}),
        ));
        assert!(matches!(arrow, Element::Arrow(_)));
        assert_eq!(arrow.binding_target(LinearEnd::Start), Some("r"));
        assert_eq!(arrow.binding_target(LinearEnd::End), None);
        let text = Element::from_value(crate::sample::text("t", [0.0, 0.0, 10.0, 10.0], "hi", Some("r")));
        assert_eq!(text.container_id(), Some("r"));
    }

    #[test]
    fn setters_write_through_typed_and_raw_paths() {
        let mut rect = Element::from_value(crate::sample::with(
            rectangle(),
            json!({"boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}], "frameId": "f"}),
        ));
        rect.set_position(7.0, 8.0);
        rect.remove_bound_element("a");
        rect.clear_frame_id();
        rect.set_deleted(true);
        let value = rect.to_value();
        assert_eq!((value["x"].clone(), value["y"].clone()), (json!(7.0), json!(8.0)));
        assert_eq!(value["boundElements"], json!([{"id": "t", "type": "text"}]));
        assert_eq!(value["frameId"], Value::Null);
        assert_eq!(value["isDeleted"], json!(true));

        let mut arrow = Element::from_value(crate::sample::with(
            crate::sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]),
            json!({"endBinding": {"elementId": "r", "fixedPoint": [0.5, 0.5], "mode": "orbit"}}),
        ));
        arrow.clear_binding(LinearEnd::End);
        assert_eq!(arrow.to_value()["endBinding"], Value::Null);
        assert_eq!(arrow.binding_target(LinearEnd::End), None);

        let mut text = Element::from_value(crate::sample::text("t", [0.0, 0.0, 10.0, 10.0], "hi", Some("r")));
        text.clear_container_id();
        assert_eq!(text.to_value()["containerId"], Value::Null);

        let mut image = Element::from_value(json!({"id": "i", "type": "image", "x": 1, "y": 2,
            "frameId": "f", "boundElements": [{"id": "a", "type": "arrow"}]}));
        image.set_position(3.0, 4.0);
        image.remove_bound_element("a");
        image.clear_frame_id();
        assert_eq!(
            image.to_value(),
            json!({"id": "i", "type": "image", "x": 3.0, "y": 4.0, "frameId": null, "boundElements": []})
        );
        let mut bare = Element::from_value(json!({"id": "b", "type": "magic"}));
        bare.set_position(1.0, 1.0);
        assert_eq!(bare.to_value(), json!({"id": "b", "type": "magic"}));
    }
```

- [ ] **Step 3：`geometry.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

    use serde_json::json;

    use super::*;
    use crate::sample;

    fn element(value: serde_json::Value) -> Element {
        Element::from_value(value)
    }

    fn assert_bounds(actual: Option<Bounds>, expected: Bounds) {
        let actual = actual.expect("bounds");
        for i in 0..4 {
            assert!((actual[i] - expected[i]).abs() < 1e-9, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn rotate_point_matches_point_rotate_rads() {
        let p = rotate_point([10.0, 0.0], [0.0, 0.0], FRAC_PI_2);
        assert!(p[0].abs() < 1e-12 && (p[1] - 10.0).abs() < 1e-12, "{p:?}");
        assert_eq!(size_from_points(&[[0.0, 0.0], [-5.0, 3.0], [10.0, -2.0]]), [15.0, 5.0]);
        assert_eq!(size_from_points(&[]), [0.0, 0.0]);
    }

    #[test]
    fn cubic_bounds_include_the_curve_extremum() {
        let b = cubic_bezier_bounds([0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]);
        assert_bounds(Some(b), [0.0, 0.0, 10.0, 7.5]);
        assert!(bounds_contain(&[0.0, 0.0, 10.0, 10.0], &[0.0, 2.0, 10.0, 10.0]));
        assert!(!bounds_contain(&[0.0, 0.0, 10.0, 10.0], &[0.0, 2.0, 10.1, 10.0]));
    }

    #[test]
    fn generic_bounds_rotate_about_the_center() {
        let rect = element(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]));
        assert_eq!(element_absolute_coords(&rect), Some(([0.0, 0.0, 100.0, 50.0], [50.0, 25.0])));
        assert_bounds(element_bounds(&rect), [0.0, 0.0, 100.0, 50.0]);

        let rotated = element(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"angle": FRAC_PI_2}),
        ));
        assert_eq!(element_absolute_coords(&rotated).unwrap().0, [0.0, 0.0, 100.0, 50.0]);
        assert_bounds(element_bounds(&rotated), [25.0, -25.0, 75.0, 75.0]);

        let ellipse = element(sample::with(
            sample::generic("ellipse", "e", [0.0, 0.0, 100.0, 50.0]),
            json!({"angle": FRAC_PI_4}),
        ));
        let half = 50.0f64.hypot(25.0) * FRAC_PI_4.cos();
        assert_bounds(element_bounds(&ellipse), [50.0 - half, 25.0 - half, 50.0 + half, 25.0 + half]);

        let diamond = element(sample::generic("diamond", "d", [0.0, 0.0, 100.0, 50.0]));
        assert_bounds(element_bounds(&diamond), [0.0, 0.0, 100.0, 50.0]);
    }

    #[test]
    fn freedraw_and_linear_bounds_come_from_points_and_curves() {
        let free = element(sample::freedraw("f", [100.0, 100.0], &[[0.0, 0.0], [10.0, -5.0], [20.0, 5.0]]));
        assert_eq!(element_absolute_coords(&free), Some(([100.0, 95.0, 120.0, 105.0], [110.0, 100.0])));

        // Roughness 0 keeps every rough.js control point on its segment, so the curve bounds
        // equal the polyline's.
        let line = element(sample::with(
            sample::linear("line", "l", [10.0, 20.0], &[[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]]),
            json!({"roughness": 0, "roundness": null}),
        ));
        let (coords, center) = element_absolute_coords(&line).expect("line coords");
        assert_bounds(Some(coords), [10.0, 20.0, 110.0, 70.0]);
        assert!((center[0] - 60.0).abs() < 1e-9 && (center[1] - 45.0).abs() < 1e-9);

        // A rounded (curved) line's bounds cover at least its points.
        let curve = element(sample::linear("line", "c", [0.0, 0.0], &[[0.0, 0.0], [50.0, 50.0], [100.0, 0.0]]));
        let b = element_bounds(&curve).expect("curve bounds");
        assert!(b[0] <= 0.0 && b[1] <= 0.0 && b[2] >= 100.0 && b[3] >= 50.0, "{b:?}");
    }

    #[test]
    fn raw_elements_use_their_placement_and_cache_follows_versions() {
        let image = element(json!({"id": "i", "type": "image", "x": 5, "y": 6, "width": 7, "height": 8, "version": 1, "versionNonce": 1}));
        assert_eq!(element_absolute_coords(&image), Some(([5.0, 6.0, 12.0, 14.0], [8.5, 10.0])));
        assert_eq!(element_absolute_coords(&element(json!({"id": "x", "type": "magic"}))), None);

        let mut cache = GeometryCache::default();
        let a = element(sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]));
        let b = element(sample::generic("ellipse", "b", [20.0, -5.0, 10.0, 10.0]));
        assert_eq!(cache.common_bounds([&a, &b]), Some([0.0, -5.0, 30.0, 10.0]));
        assert_eq!(cache.common_bounds(std::iter::empty()), None);

        let moved = element(sample::with(
            sample::generic("rectangle", "a", [50.0, 0.0, 10.0, 10.0]),
            json!({"versionNonce": 99}),
        ));
        assert_eq!(cache.bounds(&moved), Some([50.0, 0.0, 60.0, 10.0]));
    }
}
```

- [ ] **Step 4：確認失敗**

Run: `cargo test -p scene --lib element geometry`
Expected: 編譯失敗。

- [ ] **Step 5：實作**

讀：`packages/element/src/bounds.ts` 的 `getElementAbsoluteCoords`、`getFreeDrawElementAbsoluteCoords`、`ElementBounds.calculateBounds`、`getMinMaxXYFromCurvePathOps`、`getCubicBezierCurveBound`、`getCommonBounds`、`boundsContainBounds`；`packages/element/src/linearElementEditor.ts` 的 `getElementAbsoluteCoords`；`packages/utils/src/shape.ts` 的 `getCurvePathOps`；`packages/math/src/point.ts` 的 `pointRotateRads`；`packages/common/src/points.ts` 的 `getSizeFromPoints`。

- 讀寫方法：typed 元件的 `locked`、`groupIds`、`boundElements`、`startBinding`、`endBinding`、`frameId` 在 `extra` 或 `ElementBase` 裡（`groupIds` 是 `ElementBase::group_ids`），`containerId` 是 `TextElement::container_id`；`Raw` 直接讀寫 JSON 物件。`set_*` 與 `clear_*` 不呼叫 `bump_version`，由呼叫端決定。`clear_binding`、`clear_frame_id` 在 key 不存在時也寫入 `null`（Excalidraw `mutateElement` 的結果）。
- `element_absolute_coords`：rectangle／diamond／ellipse／text／`Raw` 是 `[x, y, x + width, y + height]`（`Raw` 用 `Element::placement`）；freedraw 是 points 的最小最大值加 `x`、`y`；line／arrow 用 `generate_element_shape(element, &ShapeContext { dark_mode: false, canvas_background_color: "#ffffff" })` 的第一個 drawable，取第一個 `OpSetType::Path` 的 ops（沒有就取第一個 set），照 `getMinMaxXYFromCurvePathOps` 只看 `BCurveTo`；形狀不是 `Drawables` 或算不出範圍時，退回 points 的最小最大值。中心是外框的中點。
- `element_bounds`：freedraw 旋轉每個點；line／arrow 旋轉曲線 ops 的每個控制點再取 bezier 範圍；diamond 旋轉四邊中點；ellipse 用 `hypot` 的公式；其他旋轉四個角。旋轉中心一律是 `element_absolute_coords` 的中心。三角函數照 JS 用 `f64::sin`／`cos`，`hypot` 用 `rough::js::hypot`。
- `GeometryCache`：key 是 id（沒有 id 的 `Raw` 用空字串），entry 存 `version`、`versionNonce` 的 bit pattern 與已算好的欄位；不相符就整個重算。Task 2 會在 entry 加碰撞形狀。

- [ ] **Step 6：驗證**

Run: `cargo test --workspace` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 7：Commit**

```bash
git add crates/scene crates/app
git commit -m "Add element editing accessors, geometry bounds and move sample JSON into scene"
```

---

### Task 2：點選判定

**Files:**
- Create: `crates/scene/src/collision.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/geometry.rs`（`GeometryCache` 加碰撞形狀）

**Interfaces:**
- Consumes: Task 1 的 `GeometryCache`、`rotate_point`、`Element` 讀取方法；`scene::color::is_transparent`；`rough::points_on_curve::simplify`、`RoughGenerator::curve`。
- Produces:

```rust
/// `SIDE_RESIZING_THRESHOLD`, in CSS pixels.
pub const SIDE_RESIZING_THRESHOLD: f64 = 4.0;
/// `DEFAULT_COLLISION_THRESHOLD`, in CSS pixels.
pub const DEFAULT_COLLISION_THRESHOLD: f64 = 2.0 * SIDE_RESIZING_THRESHOLD - 0.00001;
/// `LINE_CONFIRM_THRESHOLD`.
pub const LINE_CONFIRM_THRESHOLD: f64 = 8.0;

/// One piece of an element's outline in scene coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Line([[f64; 2]; 2]),
    Cubic([[f64; 2]; 4]),
}

/// `getElementHitThreshold`: `max(strokeWidth / 2 + 0.1, 0.85 * DEFAULT_COLLISION_THRESHOLD / zoom)`.
pub fn hit_threshold(element: &Element, zoom: f64) -> f64;
/// `shouldTestInside`.
pub fn should_test_inside(element: &Element) -> bool;
/// `isPathALoop`.
pub fn is_path_a_loop(points: &[[f64; 2]], zoom: f64) -> bool;
/// `hitElementItself` without the frame-name label.
pub fn hit_element_itself(geometry: &mut GeometryCache, element: &Element, point: [f64; 2], threshold: f64) -> bool;
/// `distanceToElement`.
pub fn distance_to_element(geometry: &mut GeometryCache, element: &Element, point: [f64; 2]) -> f64;
/// Whether `point` lies inside the element's closed outline (`isPointInElement`).
pub fn is_point_in_element(geometry: &mut GeometryCache, element: &Element, point: [f64; 2]) -> bool;

impl GeometryCache {
    /// `generateLinearCollisionShape` for line/arrow/freedraw, in scene coordinates.
    pub fn linear_collision_shape(&mut self, element: &Element) -> std::sync::Arc<[Segment]>;
}
```

- [ ] **Step 1：測試**

```rust
#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use serde_json::json;

    use super::*;
    use crate::sample;

    fn hit(element: &Element, point: [f64; 2], zoom: f64) -> bool {
        let mut geometry = GeometryCache::default();
        hit_element_itself(&mut geometry, element, point, hit_threshold(element, zoom))
    }

    fn rect(background: &str) -> Element {
        Element::from_value(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"backgroundColor": background}),
        ))
    }

    fn polyline(kind: &str, background: &str, points: &[[f64; 2]]) -> Element {
        Element::from_value(sample::with(
            sample::linear(kind, "l", [0.0, 0.0], points),
            json!({"backgroundColor": background, "roughness": 0, "roundness": null}),
        ))
    }

    const SQUARE: [[f64; 2]; 5] = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0], [0.0, 0.0]];

    #[test]
    fn threshold_scales_with_zoom_and_stroke_width() {
        let r = rect("transparent");
        assert!((hit_threshold(&r, 1.0) - 0.85 * DEFAULT_COLLISION_THRESHOLD).abs() < 1e-12);
        assert!((hit_threshold(&r, 4.0) - 0.85 * DEFAULT_COLLISION_THRESHOLD / 4.0).abs() < 1e-12);
        let thick = Element::from_value(sample::with(
            sample::generic("rectangle", "t", [0.0, 0.0, 10.0, 10.0]),
            json!({"strokeWidth": 20}),
        ));
        assert!((hit_threshold(&thick, 1.0) - 10.1).abs() < 1e-12);
    }

    #[test]
    fn transparent_shapes_hit_only_near_the_outline() {
        let r = rect("transparent");
        assert!(hit(&r, [50.0, 0.0], 1.0));
        assert!(hit(&r, [50.0, -5.0], 1.0));
        assert!(!hit(&r, [50.0, -10.0], 1.0));
        assert!(!hit(&r, [50.0, 50.0], 1.0));
        assert!(!hit(&r, [50.0, -5.0], 4.0));
        assert!(hit(&rect("#ffc9c9"), [50.0, 50.0], 1.0));

        let ellipse = Element::from_value(sample::generic("ellipse", "e", [0.0, 0.0, 100.0, 50.0]));
        assert!(hit(&ellipse, [50.0, 1.0], 1.0));
        assert!(hit(&ellipse, [100.0, 25.0], 1.0));
        assert!(!hit(&ellipse, [50.0, 25.0], 1.0));

        let diamond = Element::from_value(sample::generic("diamond", "d", [0.0, 0.0, 100.0, 100.0]));
        assert!(hit(&diamond, [75.0, 25.0], 1.0));
        assert!(!hit(&diamond, [50.0, 50.0], 1.0));
    }

    #[test]
    fn inside_rules_follow_should_test_inside() {
        assert!(hit(&polyline("line", "#ffc9c9", &SQUARE), [50.0, 50.0], 1.0));
        assert!(!hit(&polyline("line", "transparent", &SQUARE), [50.0, 50.0], 1.0));
        assert!(!hit(&polyline("arrow", "#ffc9c9", &SQUARE), [50.0, 50.0], 1.0));
        assert!(hit(&polyline("arrow", "#ffc9c9", &SQUARE), [50.0, 3.0], 1.0));
        // An open line never has an inside.
        let open = polyline("line", "#ffc9c9", &SQUARE[..4]);
        assert!(!hit(&open, [50.0, 50.0], 1.0));

        let text = Element::from_value(sample::text("t", [0.0, 0.0, 40.0, 25.0], "hi", None));
        assert!(hit(&text, [20.0, 12.0], 1.0));

        let labelled = Element::from_value(sample::with(
            sample::generic("rectangle", "c", [0.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ));
        assert!(hit(&labelled, [50.0, 50.0], 1.0));
    }

    #[test]
    fn raw_elements_hit_like_their_excalidraw_types() {
        let raw = |kind: &str| {
            Element::from_value(json!({"id": "x", "type": kind, "x": 0, "y": 0, "width": 100,
                "height": 100, "angle": 0, "strokeWidth": 2, "backgroundColor": "transparent",
                "version": 1, "versionNonce": 1}))
        };
        assert!(hit(&raw("image"), [50.0, 50.0], 1.0));
        assert!(hit(&raw("embeddable"), [50.0, 50.0], 1.0));
        assert!(!hit(&raw("frame"), [50.0, 50.0], 1.0));
        assert!(hit(&raw("frame"), [50.0, 1.0], 1.0));
    }

    #[test]
    fn rotation_is_applied_to_the_query_point() {
        let rotated = Element::from_value(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 20.0]),
            json!({"angle": FRAC_PI_2}),
        ));
        assert!(hit(&rotated, [40.0, 30.0], 1.0));
        assert!(!hit(&rotated, [90.0, 10.0], 1.0));
        assert!(is_path_a_loop(&SQUARE, 1.0));
        assert!(!is_path_a_loop(&SQUARE[..3], 1.0));
    }
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --lib collision`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

讀：`packages/element/src/collision.ts` 的 `shouldTestInside`、`hitElementItself`、`isPointInElement`、`isPointOnElementOutline`、`intersectElementWithLineSegment`；`packages/element/src/distance.ts` 全檔；`packages/element/src/utils.ts` 的 `deconstructRectanguloidElement`、`deconstructDiamondElement`、`deconstructLinearOrFreeDrawElement`、`getCornerRadius`、`isPathALoop`；`packages/element/src/shape.ts` 的 `generateLinearCollisionShape`；`packages/element/src/comparisons.ts` 的 `hasBackground`；`packages/math/src/ellipse.ts` 的 `ellipseDistanceFromPoint`；`packages/math/src/curve.ts` 的 `curveClosestParameter`、`curvePointDistance`；`packages/math/src/segment.ts` 的 `distanceToLineSegment`；App.tsx 的 `getElementHitThreshold`（約第 6734 行）。

- `should_test_inside` 用 `element.kind()` 的字串比對，`Raw` 與 typed 共用；`backgroundColor` 從 typed 的 base 或 `Raw` 的 JSON 讀，沒有就當透明；`hasBoundTextElement` 是 `bound_elements()` 裡有 `type == "text"`。
- 距離：rectangle、text、`Raw`（任何未知類型）用 rectanguloid；`Raw` 不讀 `roundness`，當成直角（它畫成虛線框）。ellipse 的三次迭代照抄，不改成收斂迴圈。line／arrow／freedraw 用 `linear_collision_shape`：rough 的 options 是 `seed`、`disable_multi_stroke`、`disable_multi_stroke_fill`、`roughness: 0`、`preserve_vertices`；freedraw 先 `simplify(points, 0.75)`。形狀已含旋轉，查詢點不再旋轉。
- 內部判定：`isPointInElement` 用射線與輪廓交點的奇偶。napkin 可以用等價的做法：rectanguloid、diamond、ellipse 先把點轉回未旋轉座標再用解析式判斷（圓角矩形的角落用圓弧），封閉的 line／freedraw 把碰撞形狀的 cubic 以 16 段折線近似後做奇偶射線測試。
- `hit_element_itself`：先用 `element_bounds`（未旋轉版本）加門檻做快速排除，查詢點繞中心轉 `-angle`；通過後照 `shouldTestInside` 決定 `inside || outline` 或只看 outline。

- [ ] **Step 4：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 5：Commit**

```bash
git add crates/scene
git commit -m "Port Excalidraw hit testing for element outlines and interiors"
```

---

### Task 3：選取規則

**Files:**
- Create: `crates/scene/src/selection.rs`
- Modify: `crates/scene/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 `GeometryCache`、`bounds_contain`；Task 2 `hit_element_itself`、`hit_threshold`、`DEFAULT_COLLISION_THRESHOLD`、`collision::SIDE_RESIZING_THRESHOLD`。
- Produces:

```rust
/// Selected element ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection(std::collections::BTreeSet<String>);
impl Selection {
    pub fn new() -> Selection;
    pub fn from_ids<S: Into<String>>(ids: impl IntoIterator<Item = S>) -> Selection;
    pub fn contains(&self, id: &str) -> bool;
    pub fn insert(&mut self, id: impl Into<String>) -> bool;
    pub fn remove(&mut self, id: &str) -> bool;
    pub fn is_empty(&self) -> bool;
    pub fn len(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = &str>;
    /// Positions in `file` of selected, non-deleted elements, ascending.
    pub fn positions(&self, file: &SceneFile) -> Vec<usize>;
}

/// Neither deleted, locked, nor text bound to a container (`shouldIgnoreElementFromSelection`).
pub fn is_selectable(element: &Element) -> bool;
/// `selectGroupsForSelectedElements` without `editingGroupId`.
pub fn select_groups(file: &SceneFile, selection: &Selection) -> Selection;
/// `actionSelectAll`.
pub fn select_all(file: &SceneFile) -> Selection;
/// `getElementsWithinSelection` in `"contain"` mode, group-folded.
pub fn box_select(geometry: &mut GeometryCache, file: &SceneFile, rect: Bounds) -> Selection;
/// `getElementsAtPosition`: positions of hit elements in z-order (bottom first).
pub fn elements_at(geometry: &mut GeometryCache, file: &SceneFile, point: [f64; 2], zoom: f64, selection: &Selection) -> Vec<usize>;
/// `getElementAtPosition`: topmost, re-tested at half the threshold, else the one below.
pub fn element_at(geometry: &mut GeometryCache, file: &SceneFile, point: [f64; 2], zoom: f64, selection: &Selection) -> Option<usize>;
/// `getCommonBounds` of the selected, non-deleted elements.
pub fn selected_bounds(geometry: &mut GeometryCache, file: &SceneFile, selection: &Selection) -> Option<Bounds>;
/// `isHittingCommonBoundingBoxOfSelectedElements` (two or more selected).
pub fn hits_selection_box(geometry: &mut GeometryCache, file: &SceneFile, selection: &Selection, point: [f64; 2], zoom: f64) -> bool;
```

- [ ] **Step 1：測試**

```rust
#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::sample;

    fn rect(id: &str, rect: [f64; 4]) -> Value {
        sample::generic("rectangle", id, rect)
    }

    fn at(file: &SceneFile, point: [f64; 2], selection: &Selection) -> Option<String> {
        let mut geometry = GeometryCache::default();
        element_at(&mut geometry, file, point, 1.0, selection)
            .map(|i| file.elements[i].id().unwrap().to_string())
    }

    #[test]
    fn topmost_wins_unless_its_precise_retest_fails() {
        let file = sample::file(vec![
            sample::with(rect("below", [0.0, 0.0, 100.0, 100.0]), json!({"backgroundColor": "#ffc9c9"})),
            rect("above", [0.0, 45.0, 100.0, 100.0]),
        ]);
        let none = Selection::new();
        // 5 units from `above`'s top edge: inside the threshold (6.8) but not half of it.
        assert_eq!(at(&file, [50.0, 50.0], &none).as_deref(), Some("below"));
        assert_eq!(at(&file, [50.0, 47.0], &none).as_deref(), Some("above"));
        assert_eq!(at(&file, [300.0, 300.0], &none), None);
    }

    #[test]
    fn deleted_locked_and_bound_text_are_not_hit_directly() {
        let file = sample::file(vec![
            sample::with(rect("deleted", [0.0, 0.0, 100.0, 100.0]), json!({"isDeleted": true, "backgroundColor": "#ffc9c9"})),
            sample::with(rect("locked", [200.0, 0.0, 100.0, 100.0]), json!({"locked": true, "backgroundColor": "#ffc9c9"})),
            sample::with(
                sample::with(sample::linear("arrow", "a", [0.0, 300.0], &[[0.0, 0.0], [200.0, 0.0]]), json!({"roughness": 0, "roundness": null})),
                json!({"boundElements": [{"id": "label", "type": "text"}]}),
            ),
            sample::text("label", [80.0, 290.0, 40.0, 20.0], "hi", Some("a")),
        ]);
        let none = Selection::new();
        assert_eq!(at(&file, [50.0, 50.0], &none), None);
        assert_eq!(at(&file, [250.0, 50.0], &none), None);
        // 9 units from the arrow, inside its label: the arrow is hit, never the label itself.
        assert_eq!(at(&file, [100.0, 309.0], &none).as_deref(), Some("a"));
        let mut geometry = GeometryCache::default();
        assert_eq!(elements_at(&mut geometry, &file, [100.0, 300.0], 1.0, &none), vec![2]);
    }

    #[test]
    fn a_selected_element_is_hit_anywhere_in_its_box() {
        let file = sample::file(vec![rect("r", [0.0, 0.0, 100.0, 100.0])]);
        assert_eq!(at(&file, [50.0, 50.0], &Selection::new()), None);
        assert_eq!(at(&file, [50.0, 50.0], &Selection::from_ids(["r"])).as_deref(), Some("r"));
    }

    #[test]
    fn groups_select_their_outermost_members() {
        let grouped = |id: &str, groups: &[&str]| sample::with(rect(id, [0.0, 0.0, 10.0, 10.0]), json!({"groupIds": groups}));
        let file = sample::file(vec![
            grouped("a", &["g1"]),
            grouped("b", &["g1"]),
            grouped("c", &["lonely"]),
            grouped("d", &["inner", "outer"]),
            grouped("e", &["outer"]),
            sample::with(grouped("f", &["g1"]), json!({"isDeleted": true})),
        ]);
        assert_eq!(select_groups(&file, &Selection::from_ids(["a"])), Selection::from_ids(["a", "b"]));
        assert_eq!(select_groups(&file, &Selection::from_ids(["d"])), Selection::from_ids(["d", "e"]));
        assert_eq!(select_groups(&file, &Selection::from_ids(["c"])), Selection::from_ids(["c"]));
    }

    #[test]
    fn select_all_skips_deleted_locked_and_bound_text() {
        let file = sample::file(vec![
            rect("r", [0.0, 0.0, 10.0, 10.0]),
            sample::with(rect("deleted", [0.0, 0.0, 10.0, 10.0]), json!({"isDeleted": true})),
            sample::with(rect("locked", [0.0, 0.0, 10.0, 10.0]), json!({"locked": true})),
            sample::text("bound", [0.0, 0.0, 10.0, 10.0], "hi", Some("r")),
            sample::text("free", [0.0, 0.0, 10.0, 10.0], "hi", None),
        ]);
        assert_eq!(select_all(&file), Selection::from_ids(["r", "free"]));
    }

    #[test]
    fn box_selection_needs_full_containment_including_half_the_stroke() {
        let file = sample::file(vec![
            rect("r", [10.0, 10.0, 80.0, 80.0]),
            sample::with(rect("g1", [200.0, 0.0, 10.0, 10.0]), json!({"groupIds": ["g"]})),
            sample::with(rect("g2", [300.0, 0.0, 10.0, 10.0]), json!({"groupIds": ["g"]})),
        ]);
        let mut geometry = GeometryCache::default();
        assert!(box_select(&mut geometry, &file, [10.0, 10.0, 90.0, 90.0]).is_empty());
        assert_eq!(box_select(&mut geometry, &file, [9.0, 9.0, 91.0, 91.0]), Selection::from_ids(["r"]));
        assert!(box_select(&mut geometry, &file, [190.0, -10.0, 250.0, 20.0]).is_empty());
        assert_eq!(box_select(&mut geometry, &file, [190.0, -10.0, 350.0, 20.0]), Selection::from_ids(["g1", "g2"]));
    }

    #[test]
    fn selection_box_counts_only_for_multiple_elements() {
        let file = sample::file(vec![rect("a", [0.0, 0.0, 10.0, 10.0]), rect("b", [50.0, 50.0, 10.0, 10.0])]);
        let both = Selection::from_ids(["a", "b"]);
        let mut geometry = GeometryCache::default();
        assert_eq!(selected_bounds(&mut geometry, &file, &both), Some([0.0, 0.0, 60.0, 60.0]));
        assert!(hits_selection_box(&mut geometry, &file, &both, [30.0, 30.0], 1.0));
        // Bounds grow by 4 / zoom plus max(DEFAULT_COLLISION_THRESHOLD / zoom, 1): about 72.
        assert!(hits_selection_box(&mut geometry, &file, &both, [71.0, 30.0], 1.0));
        assert!(!hits_selection_box(&mut geometry, &file, &both, [73.0, 30.0], 1.0));
        assert!(!hits_selection_box(&mut geometry, &file, &Selection::from_ids(["a"]), [5.0, 5.0], 1.0));
    }
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --lib selection`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

讀：App.tsx 的 `getElementsAtPosition`（約第 6674 行）、`getElementAtPosition`（約第 6614 行）、`hitElement`（約第 6744 行）、`isHittingCommonBoundingBoxOfSelectedElements`（約第 9864 行）；`packages/element/src/collision.ts` 的 `hitElementBoundingBox`、`hitElementBoundText`；`packages/element/src/selection.ts` 的 `getElementsWithinSelection`、`shouldIgnoreElementFromSelection`；`packages/element/src/bounds.ts` 的 `elementsOverlappingBBox`（只取 `"contain"` 路徑）；`packages/element/src/groups.ts` 的 `selectGroupsForSelectedElements`；`packages/excalidraw/actions/actionSelectAll.ts`；`packages/element/src/transformHandles.ts` 的 `hasBoundingBox`。

- `elements_at` 先建一次 id → 位置的 `HashMap`（綁定文字查詢用），不要對每個元件線性搜尋。frame 裁切（`isCursorInFrame`）不做；iframe 類排到最後照做（`embeddable`、`iframe`）。
- `hitElement` 的外框捷徑只對已選取、且 `hasBoundingBox` 成立的元件：選取兩個以上，或單選的元件不是兩點的 line／arrow。
- `hitElementBoundText` 對箭頭標籤用標籤存的 `x`、`y`（不 port `getBoundTextElementPosition`，理由見「與 roadmap、spec 不同的地方」第 7 點），判定是點落在標籤的旋轉矩形內。
- 框選不處理 frame 子元件的去重（napkin 不選取 frame 內容以外的東西）；箭頭標籤的外框併入箭頭外框照做。

- [ ] **Step 4：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 5：Commit**

```bash
git add crates/scene
git commit -m "Port Excalidraw selection rules: hit priority, groups, box and select all"
```

---

### Task 4：控制點與縮放

**Files:**
- Create: `crates/scene/src/transform.rs`
- Modify: `crates/scene/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 `GeometryCache`、`rotate_point`、`Bounds`；Task 2 `SIDE_RESIZING_THRESHOLD`；Task 3 `Selection`、`selected_bounds`；`scene::new_element::bump_version`。
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandleKind { N, S, E, W, Nw, Ne, Sw, Se }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResizeOptions {
    /// Shift (`shouldMaintainAspectRatio`).
    pub keep_aspect_ratio: bool,
    /// Alt (`shouldResizeFromCenter`).
    pub from_center: bool,
}

/// `getTransformHandlesFromCoords` at angle 0 for a mouse pointer on desktop: the four corner
/// squares in `nw, ne, sw, se` order, minus `omit`.
pub fn corner_handles(bounds: Bounds, zoom: f64, margin: f64, omit: &[HandleKind]) -> Vec<(HandleKind, Bounds)>;
/// The corner handles the selection shows: none for an empty selection, a single two-point
/// line or arrow, or a selection containing a rotated, locked, elbow-arrow or `Raw` element.
pub fn selection_handles(geometry: &mut GeometryCache, file: &SceneFile, selection: &Selection, zoom: f64) -> Vec<(HandleKind, Bounds)>;
/// `resizeTest` / `getTransformHandleTypeFromCoords`: corner squares first, then the edges of
/// the padded bounds (never `E`/`W` for a single text element, never edges for a single line
/// or arrow with two points).
pub fn handle_at(geometry: &mut GeometryCache, file: &SceneFile, selection: &Selection, point: [f64; 2], zoom: f64) -> Option<HandleKind>;
/// `getResizeOffsetXY`: `point` minus the edge or corner `handle` drags.
pub fn resize_offset(geometry: &mut GeometryCache, file: &SceneFile, selection: &Selection, handle: HandleKind, point: [f64; 2]) -> [f64; 2];
/// `resizeSingleElement` / `resizeSingleTextElement` for the element at `position`, from its
/// state in `start`, plus the position of its bound text. Returns whether anything changed.
pub fn resize_element(geometry: &mut GeometryCache, file: &mut SceneFile, start: &SceneFile, position: usize, handle: HandleKind, pointer: [f64; 2], options: ResizeOptions, env: &mut impl Env) -> bool;
/// `resizeMultipleElements` for `targets` (ascending positions), from their state in `start`.
pub fn resize_elements(geometry: &mut GeometryCache, file: &mut SceneFile, start: &SceneFile, targets: &[usize], handle: HandleKind, pointer: [f64; 2], options: ResizeOptions, env: &mut impl Env) -> bool;
/// `computeBoundTextPosition` for a rectangle, diamond or ellipse container; `None` otherwise.
pub fn bound_text_position(container: &Element, text: &TextElement) -> Option<[f64; 2]>;
```

- [ ] **Step 1：測試**

```rust
#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::sample;

    struct TestEnv;

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(7);
        }

        fn now_ms(&mut self) -> f64 {
            42.0
        }
    }

    const PLAIN: ResizeOptions = ResizeOptions { keep_aspect_ratio: false, from_center: false };

    fn resize(values: Vec<Value>, targets: &[usize], handle: HandleKind, pointer: [f64; 2], options: ResizeOptions) -> SceneFile {
        let start = sample::file(values);
        let mut file = start.clone();
        let mut geometry = GeometryCache::default();
        if let [position] = targets {
            resize_element(&mut geometry, &mut file, &start, *position, handle, pointer, options, &mut TestEnv);
        } else {
            resize_elements(&mut geometry, &mut file, &start, targets, handle, pointer, options, &mut TestEnv);
        }
        file
    }

    fn rect_of(file: &SceneFile, position: usize) -> [f64; 4] {
        let p = file.elements[position].placement().expect("placement");
        [p.x, p.y, p.width, p.height]
    }

    fn assert_rect(actual: [f64; 4], expected: [f64; 4]) {
        for i in 0..4 {
            assert!((actual[i] - expected[i]).abs() < 1e-9, "{actual:?} != {expected:?}");
        }
    }

    fn rect(id: &str, r: [f64; 4]) -> Value {
        sample::generic("rectangle", id, r)
    }

    #[test]
    fn corner_handles_follow_transform_handle_geometry() {
        use HandleKind::*;
        assert_eq!(
            corner_handles([0.0, 0.0, 100.0, 100.0], 1.0, 2.0, &[]),
            vec![
                (Nw, [-8.0, -8.0, 0.0, 0.0]),
                (Ne, [100.0, -8.0, 108.0, 0.0]),
                (Sw, [-8.0, 100.0, 0.0, 108.0]),
                (Se, [100.0, 100.0, 108.0, 108.0]),
            ]
        );
        let zoomed = corner_handles([0.0, 0.0, 100.0, 100.0], 2.0, 2.0, &[]);
        assert_eq!((zoomed[0], zoomed[3]), ((Nw, [-4.0, -4.0, 0.0, 0.0]), (Se, [100.0, 100.0, 104.0, 104.0])));
        assert_eq!(corner_handles([0.0, 0.0, 100.0, 100.0], 1.0, 10.0, &[])[0], (Nw, [-16.0, -16.0, -8.0, -8.0]));
        let slash: Vec<HandleKind> = corner_handles([0.0, 0.0, 1.0, 1.0], 1.0, 2.0, &[Nw, Se]).into_iter().map(|(k, _)| k).collect();
        assert_eq!(slash, vec![Ne, Sw]);
    }

    #[test]
    fn which_selections_have_handles_and_where_they_hit() {
        use HandleKind::*;
        let file = sample::file(vec![
            rect("r", [0.0, 0.0, 100.0, 100.0]),
            sample::with(rect("rotated", [0.0, 0.0, 10.0, 10.0]), json!({"angle": 0.3})),
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [50.0, 50.0]]),
            json!({"id": "i", "type": "image", "x": 0, "y": 0, "width": 10, "height": 10, "angle": 0, "version": 1, "versionNonce": 1}),
            sample::text("t", [300.0, 0.0, 100.0, 25.0], "hi", None),
            rect("s", [200.0, 0.0, 50.0, 50.0]),
        ]);
        let mut g = GeometryCache::default();
        let sel = |ids: &[&str]| Selection::from_ids(ids.iter().copied());

        assert_eq!(selection_handles(&mut g, &file, &sel(&["r"]), 1.0).len(), 4);
        for ids in [&["rotated"][..], &["a"][..], &["i"][..], &["r", "i"][..], &[][..]] {
            assert!(selection_handles(&mut g, &file, &sel(ids), 1.0).is_empty(), "{ids:?}");
            assert_eq!(handle_at(&mut g, &file, &sel(ids), [104.0, 104.0], 1.0), None, "{ids:?}");
        }

        let r = sel(&["r"]);
        assert_eq!(handle_at(&mut g, &file, &r, [104.0, 104.0], 1.0), Some(Se));
        assert_eq!(handle_at(&mut g, &file, &r, [102.0, -2.0], 1.0), Some(Ne));
        assert_eq!(handle_at(&mut g, &file, &r, [50.0, -3.0], 1.0), Some(N));
        assert_eq!(handle_at(&mut g, &file, &r, [-4.0, 50.0], 1.0), Some(W));
        assert_eq!(handle_at(&mut g, &file, &r, [50.0, 1.0], 1.0), None);

        let t = sel(&["t"]);
        assert_eq!(handle_at(&mut g, &file, &t, [404.0, 12.0], 1.0), None);
        assert_eq!(handle_at(&mut g, &file, &t, [350.0, 29.0], 1.0), Some(S));

        // Multiple elements: common bounds [0, 0, 250, 100] with the default margin of 4.
        let multi = selection_handles(&mut g, &file, &sel(&["r", "s"]), 1.0);
        assert_eq!(multi[0], (Nw, [-10.0, -10.0, -2.0, -2.0]));
        assert_eq!(resize_offset(&mut g, &file, &r, Se, [104.0, 103.0]), [4.0, 3.0]);
    }

    #[test]
    fn single_generic_resizes_from_the_opposite_corner() {
        use HandleKind::*;
        let r = || vec![rect("r", [0.0, 0.0, 100.0, 50.0])];
        assert_rect(rect_of(&resize(r(), &[0], Se, [150.0, 100.0], PLAIN), 0), [0.0, 0.0, 150.0, 100.0]);
        let aspect = ResizeOptions { keep_aspect_ratio: true, from_center: false };
        assert_rect(rect_of(&resize(r(), &[0], Se, [150.0, 100.0], aspect), 0), [0.0, 0.0, 200.0, 100.0]);
        let center = ResizeOptions { keep_aspect_ratio: false, from_center: true };
        assert_rect(rect_of(&resize(r(), &[0], Se, [150.0, 100.0], center), 0), [-50.0, -50.0, 200.0, 150.0]);
        assert_rect(rect_of(&resize(r(), &[0], Se, [-50.0, 100.0], PLAIN), 0), [-50.0, 0.0, 50.0, 100.0]);
        assert_rect(rect_of(&resize(r(), &[0], Nw, [20.0, 10.0], PLAIN), 0), [20.0, 10.0, 80.0, 40.0]);
        assert_rect(rect_of(&resize(r(), &[0], E, [130.0, 999.0], PLAIN), 0), [0.0, 0.0, 130.0, 50.0]);

        let resized = resize(r(), &[0], Se, [150.0, 100.0], PLAIN);
        assert_eq!(resized.elements[0].version(), 4.0);
        let untouched = resize(r(), &[0], Se, [100.0, 50.0], PLAIN);
        assert_eq!(untouched.elements[0].version(), 3.0);
    }

    #[test]
    fn lines_scale_points_and_text_scales_font_size() {
        use HandleKind::*;
        let line = sample::with(
            sample::linear("line", "l", [0.0, 0.0], &[[0.0, 0.0], [50.0, 50.0], [100.0, 0.0]]),
            json!({"roughness": 0, "roundness": null}),
        );
        let file = resize(vec![line], &[0], Se, [200.0, 100.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 200.0, 100.0]);
        assert_eq!(file.elements[0].to_value()["points"], json!([[0.0, 0.0], [100.0, 100.0], [200.0, 0.0]]));

        let text = || vec![sample::text("t", [0.0, 0.0, 100.0, 25.0], "hi", None)];
        for pointer in [[200.0, 50.0], [300.0, 50.0]] {
            let file = resize(text(), &[0], Se, pointer, PLAIN);
            assert_rect(rect_of(&file, 0), [0.0, 0.0, 200.0, 50.0]);
            assert_eq!(file.elements[0].to_value()["fontSize"], json!(40.0));
        }
        // Below MIN_FONT_SIZE nothing changes.
        let tiny = resize(text(), &[0], Se, [2.0, 1.0], PLAIN);
        assert_eq!(tiny, sample::file(text()));
    }

    #[test]
    fn multiple_elements_scale_about_the_common_bounds() {
        use HandleKind::*;
        let two = || vec![rect("a", [0.0, 0.0, 50.0, 50.0]), rect("b", [50.0, 50.0, 50.0, 50.0])];
        let file = resize(two(), &[0, 1], Se, [200.0, 200.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 100.0, 100.0]);
        assert_rect(rect_of(&file, 1), [100.0, 100.0, 100.0, 100.0]);
        let file = resize(two(), &[0, 1], Se, [200.0, 100.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 100.0, 50.0]);
        assert_rect(rect_of(&file, 1), [100.0, 50.0, 100.0, 50.0]);

        // A text element in the selection forces a uniform scale.
        let mixed = vec![rect("a", [0.0, 0.0, 50.0, 50.0]), sample::text("t", [50.0, 50.0, 100.0, 25.0], "hi", None)];
        let file = resize(mixed, &[0, 1], Se, [300.0, 75.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 100.0, 100.0]);
        assert_rect(rect_of(&file, 1), [100.0, 100.0, 200.0, 50.0]);
        assert_eq!(file.elements[1].to_value()["fontSize"], json!(40.0));
    }

    #[test]
    fn bound_text_is_repositioned_without_rewrapping() {
        let centered = |container: &str| {
            sample::with(
                sample::text("t", [30.0, 40.0, 40.0, 20.0], "hi", Some(container)),
                json!({"textAlign": "center", "verticalAlign": "middle"}),
            )
        };
        let container = sample::with(rect("r", [0.0, 0.0, 100.0, 100.0]), json!({"boundElements": [{"id": "t", "type": "text"}]}));
        let file = resize(vec![container, centered("r")], &[0], HandleKind::Se, [200.0, 100.0], PLAIN);
        assert_rect(rect_of(&file, 1), [80.0, 40.0, 40.0, 20.0]);

        let Element::Text(text) = Element::from_value(centered("c")) else { panic!("text") };
        let diamond = Element::from_value(sample::generic("diamond", "c", [0.0, 0.0, 200.0, 100.0]));
        assert_eq!(bound_text_position(&diamond, &text), Some([80.0, 40.0]));
        let ellipse = Element::from_value(sample::generic("ellipse", "c", [0.0, 0.0, 200.0, 100.0]));
        let [x, y] = bound_text_position(&ellipse, &text).expect("ellipse container");
        assert!((x - 79.789_321_881_345_24).abs() < 1e-9 && (y - 40.144_660_940_672_62).abs() < 1e-9, "{x} {y}");
        let line = Element::from_value(sample::linear("line", "c", [0.0, 0.0], &[[0.0, 0.0], [1.0, 1.0]]));
        assert_eq!(bound_text_position(&line, &text), None);
    }
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --lib transform`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

讀：`packages/element/src/transformHandles.ts` 的 `getTransformHandlesFromCoords`、`getTransformHandles`、`OMIT_SIDES_FOR_LINE_SLASH`／`BACKSLASH`、`hasBoundingBox`；`packages/element/src/resizeTest.ts` 全檔；`packages/element/src/resizeElements.ts` 的 `getResizeOffsetXY`、`resizeSingleElement`、`resizeSingleTextElement`、`getResizedOrigin`、`getResizeAnchor`、`getNextSingleWidthAndHeightFromPointer`、`resizeMultipleElements`、`getNextMultipleWidthAndHeightFromPointer`、`rescalePointsInElement`、`measureFontSizeFromWidth`；`packages/common/src/points.ts` 的 `rescalePoints`；`packages/element/src/textElement.ts` 的 `computeBoundTextPosition`、`getContainerCoords`、`getBoundTextMaxWidth`、`getBoundTextMaxHeight`；App.tsx 的 `maybeHandleResize`（約第 13833 行）。

- 單選的控制點用 `element_absolute_coords`，margin：line／arrow 10，其他 2；兩點的 freedraw 照 `getTransformHandles` 選 slash 或 backslash 的 omit。多選用 `selected_bounds`、margin 4。elbow arrow（`elbowed: true`）不能縮放（spec §1.2），選取裡有它就沒有控制點。
- 邊的判定：外框向外擴 `SIDE_RESIZING_THRESHOLD / zoom`，四條邊依 `n, e, s, w` 順序，容許距離 `SIDE_RESIZING_THRESHOLD / zoom`。
- `resize_element`：`start` 是指標按下時的場景，`file` 是目前的場景；每次都從 `start` 的元件重算。line／arrow 的 `previousOrigin` 用 `element_bounds` 的左上角。只 port `angle == 0` 會走到的分支，但 `getResizedOrigin` 的三角函數項照抄（angle 為 0 時結果相同）。任何寬或高算出 0 時不改。文字的 `E`／`W` 直接回傳 `false`。
- `resize_elements`：`keepAspectRatio` 在 Shift、任一元件是文字、或任一元件在群組裡時成立。文字字級低於 `MIN_FONT_SIZE`（1）時整個操作不改任何元件。
- 綁定文字：縮放後對每個被縮放的 rectangle／diamond／ellipse 容器，找 `bound_elements()` 裡 `type == "text"` 且未刪除的文字元件，用 `bound_text_position` 設 `x`、`y`。`Math.round` 用 `rough::js::math_round`。`resizeSingleElement` 在有綁定文字時用字寬算的最小尺寸（`getApproxMinLineWidth`）需要量字，不做，留給 M5。
- 每個有改變的元件（含綁定文字）呼叫一次 `bump_version`。

- [ ] **Step 4：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 5：Commit**

```bash
git add crates/scene
git commit -m "Port Excalidraw transform handles and single and multiple element resizing"
```

---

### Task 5：拖動、刪除、拖點與載入修復

**Files:**
- Create: `crates/scene/src/edit.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/fractional_index.rs`

**Interfaces:**
- Consumes: Task 1 讀寫方法、`size_from_points`、`rotate_point`；Task 3 `Selection`；`fractional_index::{sync_moved_indices, sync_invalid_indices}`；`env::random_id`；`bump_version`。
- Produces:

```rust
/// `dragSelectedElements`' Shift lock: the axis that moved less stays put.
pub fn lock_drag_axis(offset: [f64; 2]) -> [f64; 2];
/// Positions a drag of `selection` moves, ascending: the selected elements, elements whose
/// `frameId` is a selected frame, text bound to a moved element, and labels of moved arrows.
pub fn drag_targets(file: &SceneFile, selection: &Selection) -> Vec<usize>;
/// Puts each target at its position in `start` plus `offset`.
pub fn apply_drag(file: &mut SceneFile, start: &SceneFile, targets: &[usize], offset: [f64; 2], env: &mut impl Env);
/// `actionDeleteSelected` + `fixBindingsAfterDeletion`. Returns the selection afterwards: the
/// former children of deleted frames.
pub fn delete_selection(file: &mut SceneFile, selection: &Selection, env: &mut impl Env) -> Selection;
/// `getLockedLinearCursorAlignSize` without a custom angle: `point` snapped to 15° steps around `origin`.
pub fn lock_linear_angle(origin: [f64; 2], point: [f64; 2]) -> [f64; 2];
/// `LinearElementEditor.movePoints` for one point of a line or arrow, from its state in
/// `start`; `target` is the new point in scene coordinates.
pub fn move_linear_point(start: &Element, element: &mut Element, index: usize, target: [f64; 2], shift: bool, env: &mut impl Env);
/// `insertNewElements` without frames: appends `element`, syncs its index, returns its position.
pub fn append_element(file: &mut SceneFile, element: Element, env: &mut impl Env) -> usize;
/// The repairs `restoreElements` always applies: duplicate ids get a new nanoid (first one
/// keeps its id) and `syncInvalidIndices`.
pub fn repair_on_load(file: &mut SceneFile, env: &mut impl Env);
```

- [ ] **Step 1：`edit.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::element::LinearEnd;
    use crate::sample;

    struct TestEnv(u8);

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            for b in bytes {
                self.0 = self.0.wrapping_add(37);
                *b = self.0;
            }
        }

        fn now_ms(&mut self) -> f64 {
            42.0
        }
    }

    fn rect(id: &str, r: [f64; 4]) -> Value {
        sample::generic("rectangle", id, r)
    }

    fn xy(file: &SceneFile, id: &str) -> (f64, f64) {
        let e = file.elements.iter().find(|e| e.id() == Some(id)).expect(id);
        let p = e.placement().expect("placement");
        (p.x, p.y)
    }

    fn get<'a>(file: &'a SceneFile, id: &str) -> &'a Element {
        file.elements.iter().find(|e| e.id() == Some(id)).expect(id)
    }

    fn bindings_scene() -> SceneFile {
        sample::file(vec![
            sample::with(rect("r", [0.0, 0.0, 100.0, 100.0]), json!({"boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}]})),
            sample::text("t", [30.0, 40.0, 40.0, 20.0], "hi", Some("r")),
            sample::with(
                sample::linear("arrow", "a", [0.0, 200.0], &[[0.0, 0.0], [100.0, 0.0]]),
                json!({
                    "boundElements": [{"id": "l", "type": "text"}],
                    "startBinding": {"elementId": "r", "fixedPoint": [0.5, 1.0], "mode": "orbit"},
                    "endBinding": {"elementId": "s", "fixedPoint": [0.0, 0.5], "mode": "orbit"}
                }),
            ),
            sample::text("l", [40.0, 190.0, 20.0, 20.0], "label", Some("a")),
            sample::with(rect("s", [300.0, 150.0, 50.0, 50.0]), json!({"boundElements": [{"id": "a", "type": "arrow"}]})),
            json!({"id": "f", "type": "frame", "x": 500, "y": 0, "width": 100, "height": 100, "angle": 0,
                   "isDeleted": false, "version": 1, "versionNonce": 1}),
            sample::with(rect("c", [510.0, 10.0, 10.0, 10.0]), json!({"frameId": "f"})),
            rect("other", [900.0, 900.0, 10.0, 10.0]),
        ])
    }

    #[test]
    fn shift_lock_keeps_the_longer_axis() {
        assert_eq!(lock_drag_axis([3.0, -10.0]), [0.0, -10.0]);
        assert_eq!(lock_drag_axis([10.0, 4.0]), [10.0, 0.0]);
        assert_eq!(lock_drag_axis([5.0, -5.0]), [5.0, -5.0]);
    }

    #[test]
    fn dragging_moves_dependents_once() {
        let start = bindings_scene();
        let selection = Selection::from_ids(["r", "a", "f", "t"]);
        let targets = drag_targets(&start, &selection);
        assert_eq!(targets, vec![0, 1, 2, 3, 5, 6]);
        let mut file = start.clone();
        apply_drag(&mut file, &start, &targets, [10.0, 5.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "r"), (10.0, 5.0));
        assert_eq!(xy(&file, "t"), (40.0, 45.0));
        assert_eq!(xy(&file, "a"), (10.0, 205.0));
        assert_eq!(xy(&file, "l"), (50.0, 195.0));
        assert_eq!(xy(&file, "f"), (510.0, 5.0));
        assert_eq!(xy(&file, "c"), (520.0, 15.0));
        assert_eq!(get(&file, "other"), get(&start, "other"));
        assert_eq!(get(&file, "r").version(), get(&start, "r").version() + 1.0);
        // Re-applying from the same start is idempotent, not cumulative.
        apply_drag(&mut file, &start, &targets, [10.0, 5.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "r"), (10.0, 5.0));
    }

    #[test]
    fn deleting_a_container_takes_its_text_and_unbinds_arrows() {
        let mut file = bindings_scene();
        let next = delete_selection(&mut file, &Selection::from_ids(["r"]), &mut TestEnv(0));
        assert!(next.is_empty());
        assert!(get(&file, "r").is_deleted() && get(&file, "t").is_deleted());
        assert!(!get(&file, "a").is_deleted());
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), None);
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), Some("s"));
    }

    #[test]
    fn deleting_an_arrow_takes_its_label_and_cleans_bound_elements() {
        let mut file = bindings_scene();
        delete_selection(&mut file, &Selection::from_ids(["a"]), &mut TestEnv(0));
        assert!(get(&file, "a").is_deleted() && get(&file, "l").is_deleted());
        assert_eq!(get(&file, "r").bound_elements(), vec![("t", "text")]);
        assert_eq!(get(&file, "s").bound_elements(), vec![]);
    }

    #[test]
    fn deleting_a_frame_releases_and_selects_its_children() {
        let mut file = bindings_scene();
        let next = delete_selection(&mut file, &Selection::from_ids(["f"]), &mut TestEnv(0));
        assert_eq!(next, Selection::from_ids(["c"]));
        assert!(get(&file, "f").is_deleted() && !get(&file, "c").is_deleted());
        assert_eq!(get(&file, "c").frame_id(), None);
    }

    #[test]
    fn moving_a_point_renormalizes_the_first_point() {
        let arrow = Element::from_value(sample::linear("arrow", "a", [10.0, 10.0], &[[0.0, 0.0], [100.0, 0.0]]));
        let mut moved = arrow.clone();
        move_linear_point(&arrow, &mut moved, 0, [0.0, 30.0], false, &mut TestEnv(0));
        let v = moved.to_value();
        assert_eq!((v["x"].clone(), v["y"].clone()), (json!(0.0), json!(30.0)));
        assert_eq!(v["points"], json!([[0.0, 0.0], [110.0, -20.0]]));
        assert_eq!((v["width"].clone(), v["height"].clone()), (json!(110.0), json!(20.0)));

        let mut moved = arrow.clone();
        move_linear_point(&arrow, &mut moved, 1, [50.0, 60.0], false, &mut TestEnv(0));
        assert_eq!(moved.to_value()["points"], json!([[0.0, 0.0], [40.0, 50.0]]));

        let mut locked = arrow.clone();
        move_linear_point(&arrow, &mut locked, 1, [110.0, 13.0], true, &mut TestEnv(0));
        let points = locked.to_value()["points"].clone();
        assert!((points[1][0].as_f64().unwrap() - 100.0).abs() < 1e-9 && points[1][1].as_f64().unwrap().abs() < 1e-9, "{points}");
        let snapped = lock_linear_angle([0.0, 0.0], [10.0, 9.0]);
        assert!((snapped[0] - snapped[1]).abs() < 1e-9, "{snapped:?}");
    }

    #[test]
    fn appended_elements_sort_after_everything() {
        let mut file = sample::file(vec![rect("a", [0.0, 0.0, 1.0, 1.0]), sample::with(rect("b", [0.0, 0.0, 1.0, 1.0]), json!({"index": "a1"}))]);
        let new = Element::from_value(sample::with(rect("n", [0.0, 0.0, 1.0, 1.0]), json!({"index": null})));
        assert_eq!(append_element(&mut file, new, &mut TestEnv(0)), 2);
        let index = file.elements[2].index().expect("index").to_string();
        assert!(index.as_str() > "a1", "{index}");
    }

    #[test]
    fn load_repairs_rename_duplicates_and_sync_indices() {
        let mut file = sample::file(vec![rect("x", [0.0, 0.0, 1.0, 1.0]), rect("x", [0.0, 0.0, 1.0, 1.0]), rect("y", [0.0, 0.0, 1.0, 1.0])]);
        repair_on_load(&mut file, &mut TestEnv(0));
        let ids: Vec<&str> = file.elements.iter().map(|e| e.id().unwrap()).collect();
        assert_eq!(ids[0], "x");
        assert!(ids[1] != "x" && ids[1] != "y" && ids[1].len() == 21, "{ids:?}");
        assert_eq!(ids[2], "y");
        let indices: Vec<&str> = file.elements.iter().map(|e| e.index().unwrap()).collect();
        assert!(indices.windows(2).all(|w| w[0] < w[1]), "{indices:?}");
    }
}
```

- [ ] **Step 2：`fractional_index.rs` 的測試**

`fractional_index.rs` 目前沒有單元測試模組（它的測試是 `tests/baseline.rs` 的基準），新增一個：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct FixedEnv;

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(3);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    #[test]
    fn midpoint_handles_long_digit_runs() {
        // 200 000 trailing `z` digits: one stack frame per digit would overflow the 2 MiB
        // test-thread stack.
        let long = format!("a1{}", "z".repeat(200_000));
        let key = generate_key_between(Some(&long), Some("a2")).expect("key between");
        assert!(key.as_str() > long.as_str() && key.as_str() < "a2");
    }

    #[test]
    fn reassigning_all_indices_orders_every_element() {
        let mut elements: Vec<Element> = ["c", "b", "a"]
            .iter()
            .map(|id| {
                Element::from_value(crate::sample::with(
                    crate::sample::generic("rectangle", id, [0.0, 0.0, 1.0, 1.0]),
                    serde_json::json!({"index": "zz"}),
                ))
            })
            .collect();
        reassign_all_indices(&mut elements, &mut FixedEnv);
        let indices: Vec<&str> = elements.iter().map(|e| e.index().unwrap()).collect();
        assert!(indices.windows(2).all(|w| w[0] < w[1]), "{indices:?}");
    }
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p scene --lib edit fractional_index`
Expected: 編譯失敗。

- [ ] **Step 4：實作**

讀：`packages/element/src/dragElements.ts` 的 `dragSelectedElements`、`updateElementCoords`；App.tsx 拖動的 Shift 鎖定（約第 11049 行）；`packages/excalidraw/actions/actionDeleteSelected.tsx` 的 `deleteSelectedElements`；`packages/element/src/binding.ts` 的 `fixBindingsAfterDeletion`、`BoundElement.unbindAffected`、`BindableElement.unbindAffected`；`packages/element/src/linearElementEditor.ts` 的 `movePoints`、`_updatePoints`、`_getShiftLockedDelta`；`packages/element/src/sizeHelpers.ts` 的 `getLockedLinearCursorAlignSize`；App.tsx 的 `insertNewElements`；`packages/element/src/Scene.ts` 的 `insertElementsAtIndex`；`packages/excalidraw/data/restore.ts` 的 `restoreElements`（只取重複 id 與 `syncInvalidIndices`）；`packages/fractional-indexing/src/index.ts` 的 `midpoint`。

- `drag_targets`：先建 id → 位置的 `HashMap`；綁定文字從被移動元件的 `bound_elements()` 找 `type == "text"`，不論該元件是不是箭頭（箭頭標籤平移是 napkin 的決定，見「與 roadmap、spec 不同的地方」第 7 點）；刪除的元件不列入。
- `apply_drag`：`Raw` 的 `set_position` 在 `x`／`y` 不是數字時不動，那種元件也不 `bump_version`。
- `delete_selection`：被選的元件與「容器被選到」的文字設 `isDeleted: true`；被刪除 frame 的子元件不刪，`frameId` 設 `null` 並成為新的選取（綁定文字的話改選它的容器）。接著對每個本次刪除的元件套用 `fixBindingsAfterDeletion` 的兩個方向，對象已刪除時跳過。`startBinding`、`endBinding`、`containerId`、`boundElements` 只處理 typed 元件與 `Raw` 的 JSON，不新增 key。
- `move_linear_point`：`target` 轉成相對 `start` 的區域座標；Shift 時以相鄰點（`index == 0` 用點 1，否則用 `index - 1`）為原點鎖 15°；新的點陣列全部減去新的點 0，`x`、`y` 加上點 0 的位移（angle 為 0 時就是直接相加，但照 `_updatePoints` 寫旋轉公式），寬高用 `size_from_points`。
- `lock_linear_angle`：`atan2` 用 `rough::js::atan2`，`Math.round` 用 `rough::js::math_round`，`SHIFT_LOCKING_ANGLE = PI / 12`。
- `append_element`：推進 `file.elements`，對它的 id 呼叫 `sync_moved_indices`。
- `repair_on_load`：重複 id 換成 `env::random_id`，然後 `sync_invalid_indices`。
- `fractional_index::midpoint` 改成迴圈累積前綴，行為不變，doc comment 裡關於遞迴深度的說明拿掉。`sync_invalid_indices` 的 `expect` 改成：`generate_indices` 失敗時呼叫新的私有函數 `reassign_all_indices`，用 `generate_n_keys_between(None, None, elements.len())` 依陣列順序重新給每個元件 index（有改變的呼叫 `bump_version`）。doc comment 說明這是 napkin 在 JS 會丟例外的資料上的退路。

- [ ] **Step 5：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過，包括 M2 的 fractional index 基準。

- [ ] **Step 6：Commit**

```bash
git add crates/scene
git commit -m "Port dragging, deletion with binding cleanup, point dragging and load repairs"
```

---

### Task 6：undo 歷史

**Files:**
- Create: `crates/scene/src/history.rs`
- Modify: `crates/scene/src/lib.rs`

**Interfaces:**
- Consumes: Task 3 `Selection`。
- Produces:

```rust
/// Spec §5.7.
pub const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Default)]
pub struct History { /* undo: VecDeque<Entry>, redo: Vec<Entry> */ }

impl History {
    /// Records the element changes from `before` to `after`, matched by position; a length
    /// difference is an insertion or removal at the end. Returns `false` (recording nothing,
    /// keeping redo) when no element changed. Otherwise clears redo and drops the oldest entry
    /// beyond `HISTORY_LIMIT`.
    pub fn record(&mut self, before: &SceneFile, after: &SceneFile, selection_before: &Selection, selection_after: &Selection) -> bool;
    /// Restores the elements of the newest entry and returns its `selection_before`.
    pub fn undo(&mut self, file: &mut SceneFile) -> Option<Selection>;
    /// Reapplies the newest undone entry and returns its `selection_after`.
    pub fn redo(&mut self, file: &mut SceneFile) -> Option<Selection>;
    pub fn can_undo(&self) -> bool;
    pub fn can_redo(&self) -> bool;
    pub fn clear(&mut self);
}
```

- [ ] **Step 1：測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    fn at(x: f64) -> SceneFile {
        sample::file(vec![sample::generic("rectangle", "r", [x, 0.0, 10.0, 10.0])])
    }

    #[test]
    fn undo_and_redo_swap_states_and_selection() {
        let (before, after) = (at(0.0), at(10.0));
        let mut history = History::default();
        assert!(history.record(&before, &after, &Selection::new(), &Selection::from_ids(["r"])));
        let mut file = after.clone();
        assert_eq!(history.undo(&mut file), Some(Selection::new()));
        assert_eq!(file, before);
        assert!(!history.can_undo() && history.can_redo());
        assert_eq!(history.redo(&mut file), Some(Selection::from_ids(["r"])));
        assert_eq!(file, after);
        assert_eq!(history.redo(&mut file), None);
    }

    #[test]
    fn appended_elements_disappear_on_undo() {
        let before = at(0.0);
        let mut after = before.clone();
        after.elements.push(scene_element("n"));
        let mut history = History::default();
        history.record(&before, &after, &Selection::new(), &Selection::from_ids(["n"]));
        let mut file = after.clone();
        history.undo(&mut file);
        assert_eq!(file, before);
        history.redo(&mut file);
        assert_eq!(file, after);
    }

    fn scene_element(id: &str) -> crate::Element {
        crate::Element::from_value(sample::generic("ellipse", id, [0.0, 0.0, 5.0, 5.0]))
    }

    #[test]
    fn no_change_records_nothing_and_new_records_clear_redo() {
        let mut history = History::default();
        assert!(!history.record(&at(0.0), &at(0.0), &Selection::new(), &Selection::from_ids(["r"])));
        assert!(!history.can_undo());
        history.record(&at(0.0), &at(1.0), &Selection::new(), &Selection::new());
        let mut file = at(1.0);
        history.undo(&mut file);
        assert!(history.can_redo());
        history.record(&at(0.0), &at(2.0), &Selection::new(), &Selection::new());
        assert!(!history.can_redo());
    }

    #[test]
    fn keeps_the_latest_two_hundred_steps() {
        let mut history = History::default();
        for i in 0..(HISTORY_LIMIT + 5) {
            history.record(&at(i as f64), &at(i as f64 + 1.0), &Selection::new(), &Selection::new());
        }
        let mut file = at((HISTORY_LIMIT + 5) as f64);
        let mut undone = 0;
        while history.undo(&mut file).is_some() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY_LIMIT);
        assert_eq!(file, at(5.0));
    }
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --lib history`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

- `Entry` 存 `changes: Vec<Change>`（`position`、`before: Option<Element>`、`after: Option<Element>`）與兩個 `Selection`。比對用 `Element` 的 `PartialEq`。
- `undo`：位置小於兩邊長度的改回 `before`；`after` 比 `before` 長的部分 truncate；`before` 比較長的部分 push 回去。`redo` 反過來。
- 只記元件，不記 `appState`（視角由 app 管，不進 undo）。

- [ ] **Step 4：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 5：Commit**

```bash
git add crates/scene
git commit -m "Add a 200-step element history for undo and redo"
```

---

### Task 7：`Editor` 與選取工具

**Files:**
- Create: `crates/scene/src/editor/mod.rs`、`crates/scene/src/editor/select.rs`、`crates/scene/tests/support/mod.rs`、`crates/scene/tests/editor_select.rs`
- Modify: `crates/scene/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 到 6 的全部公開函數。
- Produces:

```rust
// editor/mod.rs
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool { #[default] Selection, Hand, Rectangle, Diamond, Ellipse, Arrow, Line, Freedraw }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers { pub shift: bool, pub alt: bool, pub ctrl: bool }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    /// Scene coordinates.
    pub at: [f64; 2],
    pub modifiers: Modifiers,
    /// The camera zoom when the event happened; CSS-pixel thresholds are divided by it.
    pub zoom: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Delete or Backspace.
    Delete,
    SelectAll,
    Undo,
    Redo,
    /// Spec §7.5: finish or discard the shape being drawn, else clear the selection.
    Escape,
    /// Enter: finish a multi-point line or arrow.
    Finalize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor { #[default] Default, Move, Crosshair, Pointer, ResizeNwse, ResizeNesw, ResizeNs, ResizeEw }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    /// One solid outline per selected element: the corners of its absolute-coords box padded by
    /// 4 / zoom, rotated with the element (`renderSelectionBorder`). None for a single
    /// two-point line or arrow.
    pub outlines: Vec<[[f64; 2]; 4]>,
    /// Two or more selected: the common bounds padded by 4 / zoom, drawn dashed.
    pub selection_box: Option<Bounds>,
    /// Corner handle squares (`transform::selection_handles`).
    pub handles: Vec<Bounds>,
    /// A single selected line or non-elbow arrow: its points in scene coordinates.
    pub points: Vec<[f64; 2]>,
    /// The rubber band while box selecting, normalized.
    pub box_selection: Option<Bounds>,
}

pub struct Editor<E: Env> { /* file, env, tool, selection, gesture, history, geometry, revision, scene_clones, cursor */ }

impl<E: Env> Editor<E> {
    /// Applies `edit::repair_on_load`. Revision starts at 0.
    pub fn new(file: SceneFile, env: E) -> Editor<E>;
    pub fn file(&self) -> &Arc<SceneFile>;
    /// Replaces the scene after an external change: repairs it, clears the selection, the
    /// history and any gesture. Does not change `revision`.
    pub fn replace_file(&mut self, file: SceneFile);
    /// Increases every time elements change, including undo and redo.
    pub fn revision(&self) -> u64;
    /// How many edits had to copy the whole scene because another `Arc` shared it.
    pub fn scene_clones(&self) -> u64;
    pub fn tool(&self) -> Tool;
    /// Finishes a multi-point line or arrow first. Any tool other than Selection and Hand
    /// clears the selection (`clearSelectionIfNotUsingSelection`).
    pub fn set_tool(&mut self, tool: Tool);
    pub fn selection(&self) -> &Selection;
    /// No pointer gesture and no multi-point line in progress.
    pub fn is_idle(&self) -> bool;
    pub fn pointer_down(&mut self, event: PointerEvent);
    pub fn pointer_move(&mut self, event: PointerEvent);
    pub fn pointer_up(&mut self, event: PointerEvent);
    /// Whether the command did anything. Commands other than `Escape` are ignored while a
    /// pointer gesture is in progress.
    pub fn command(&mut self, command: Command) -> bool;
    /// For the latest pointer position.
    pub fn cursor(&self) -> Cursor;
    pub fn overlay(&mut self, zoom: f64) -> Overlay;
}
```

- [ ] **Step 1：測試輔助程式 `crates/scene/tests/support/mod.rs`**

```rust
//! Shared helpers for the editor interaction tests.

#![allow(dead_code)]

use scene::editor::{Editor, Modifiers, PointerEvent};
use scene::env::Env;
use scene::{Element, sample};
use serde_json::Value;

/// xorshift64 random bytes and a clock that advances 1 ms per read.
pub struct TestEnv {
    state: u64,
    now: f64,
}

impl TestEnv {
    pub fn seeded(seed: u64) -> TestEnv {
        TestEnv { state: seed.max(1), now: 1_700_000_000_000.0 }
    }
}

impl Env for TestEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            *byte = (self.state >> 32) as u8;
        }
    }

    fn now_ms(&mut self) -> f64 {
        self.now += 1.0;
        self.now
    }
}

pub fn editor(elements: Vec<Value>) -> Editor<TestEnv> {
    Editor::new(sample::file(elements), TestEnv::seeded(1))
}

pub fn at(x: f64, y: f64) -> PointerEvent {
    PointerEvent { at: [x, y], modifiers: Modifiers::default(), zoom: 1.0 }
}

pub fn shift(x: f64, y: f64) -> PointerEvent {
    PointerEvent { modifiers: Modifiers { shift: true, ..Modifiers::default() }, ..at(x, y) }
}

pub fn alt(x: f64, y: f64) -> PointerEvent {
    PointerEvent { modifiers: Modifiers { alt: true, ..Modifiers::default() }, ..at(x, y) }
}

pub fn click(editor: &mut Editor<TestEnv>, event: PointerEvent) {
    editor.pointer_down(event);
    editor.pointer_up(event);
}

/// Presses at `from`, moves to `to` in four equal steps and releases there, keeping `from`'s
/// modifiers and zoom.
pub fn drag(editor: &mut Editor<TestEnv>, from: PointerEvent, to: [f64; 2]) {
    editor.pointer_down(from);
    for step in 1..=4 {
        let t = f64::from(step) / 4.0;
        let at = [from.at[0] + (to[0] - from.at[0]) * t, from.at[1] + (to[1] - from.at[1]) * t];
        editor.pointer_move(PointerEvent { at, ..from });
    }
    editor.pointer_up(PointerEvent { at: to, ..from });
}

pub fn element<'a>(editor: &'a Editor<TestEnv>, id: &str) -> &'a Element {
    editor
        .file()
        .elements
        .iter()
        .find(|e| e.id() == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}

/// `[x, y, width, height]`.
pub fn rect_of(editor: &Editor<TestEnv>, id: &str) -> [f64; 4] {
    let p = element(editor, id).placement().expect("placement");
    [p.x, p.y, p.width, p.height]
}

pub fn selected(editor: &Editor<TestEnv>) -> Vec<String> {
    editor.selection().iter().map(str::to_string).collect()
}
```

- [ ] **Step 2：`crates/scene/tests/editor_select.rs`**

```rust
mod support;

use scene::editor::{Command, Cursor, Tool};
use scene::sample;
use serde_json::{Value, json};
use support::*;

fn solid(id: &str, r: [f64; 4]) -> Value {
    sample::with(sample::generic("rectangle", id, r), json!({"backgroundColor": "#ffc9c9"}))
}

#[test]
fn click_selects_the_hit_element_and_empty_space_clears() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 100.0]), solid("b", [200.0, 0.0, 100.0, 100.0])]);
    click(&mut e, at(50.0, 50.0));
    assert_eq!(selected(&e), ["a"]);
    click(&mut e, at(250.0, 50.0));
    assert_eq!(selected(&e), ["b"]);
    click(&mut e, at(500.0, 500.0));
    assert!(selected(&e).is_empty());
    assert_eq!(e.revision(), 0, "selection changes are not edits");
}

#[test]
fn shift_click_adds_and_removes_on_release() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 100.0]), solid("b", [200.0, 0.0, 100.0, 100.0])]);
    click(&mut e, at(50.0, 50.0));
    click(&mut e, shift(250.0, 50.0));
    assert_eq!(selected(&e), ["a", "b"]);
    e.pointer_down(shift(50.0, 50.0));
    assert_eq!(selected(&e), ["a", "b"], "removal waits for pointer up");
    e.pointer_up(shift(50.0, 50.0));
    assert_eq!(selected(&e), ["b"]);
}

#[test]
fn clicking_one_of_several_selected_elements_narrows_the_selection() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 100.0]), solid("b", [200.0, 0.0, 100.0, 100.0])]);
    assert!(e.command(Command::SelectAll));
    click(&mut e, at(50.0, 50.0));
    assert_eq!(selected(&e), ["a"]);
}

#[test]
fn clicking_a_group_member_selects_the_group() {
    let grouped = |id, r| sample::with(solid(id, r), json!({"groupIds": ["g"]}));
    let mut e = editor(vec![grouped("a", [0.0, 0.0, 50.0, 50.0]), grouped("b", [100.0, 0.0, 50.0, 50.0])]);
    click(&mut e, at(25.0, 25.0));
    assert_eq!(selected(&e), ["a", "b"]);
}

#[test]
fn box_selection_updates_while_dragging() {
    let mut e = editor(vec![solid("a", [10.0, 10.0, 50.0, 50.0]), solid("b", [100.0, 100.0, 50.0, 50.0])]);
    e.pointer_down(at(0.0, 0.0));
    e.pointer_move(at(70.0, 70.0));
    assert_eq!(selected(&e), ["a"]);
    assert_eq!(e.overlay(1.0).box_selection, Some([0.0, 0.0, 70.0, 70.0]));
    e.pointer_move(at(200.0, 200.0));
    assert_eq!(selected(&e), ["a", "b"]);
    e.pointer_up(at(200.0, 200.0));
    assert_eq!(selected(&e), ["a", "b"]);
    assert_eq!(e.overlay(1.0).box_selection, None);
    assert_eq!(e.revision(), 0);
}

#[test]
fn dragging_moves_bound_text_and_undoes_in_one_step() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::text("t", [30.0, 40.0, 40.0, 20.0], "hi", Some("r")),
    ]);
    drag(&mut e, at(50.0, 50.0), [80.0, 70.0]);
    assert_eq!(selected(&e), ["r"]);
    assert_eq!(rect_of(&e, "r")[..2], [30.0, 20.0]);
    assert_eq!(rect_of(&e, "t")[..2], [60.0, 60.0]);
    let moved = e.file().as_ref().clone();

    assert!(e.command(Command::Undo));
    assert_eq!(rect_of(&e, "r")[..2], [0.0, 0.0]);
    assert_eq!(rect_of(&e, "t")[..2], [30.0, 40.0]);
    assert!(selected(&e).is_empty());
    assert!(e.command(Command::Redo));
    assert_eq!(*e.file().as_ref(), moved);
    assert_eq!(selected(&e), ["r"]);
    assert!(!e.command(Command::Redo));
}

#[test]
fn shift_drag_locks_to_the_longer_axis() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 100.0])]);
    drag(&mut e, shift(50.0, 50.0), [90.0, 60.0]);
    assert_eq!(rect_of(&e, "a")[..2], [40.0, 0.0]);
}

#[test]
fn dragging_inside_the_box_of_a_multi_selection_moves_everything() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 50.0, 50.0]), solid("b", [100.0, 100.0, 50.0, 50.0])]);
    e.command(Command::SelectAll);
    drag(&mut e, at(75.0, 75.0), [85.0, 85.0]);
    assert_eq!(rect_of(&e, "a")[..2], [10.0, 10.0]);
    assert_eq!(rect_of(&e, "b")[..2], [110.0, 110.0]);
}

#[test]
fn corner_handle_resizes_and_shows_a_resize_cursor() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 50.0])]);
    click(&mut e, at(50.0, 25.0));
    e.pointer_move(at(104.0, 54.0));
    assert_eq!(e.cursor(), Cursor::ResizeNwse);
    // Grabbed 4 units past the corner: the offset is kept, so the corner ends at (150, 100).
    drag(&mut e, at(104.0, 54.0), [154.0, 104.0]);
    assert_eq!(rect_of(&e, "a"), [0.0, 0.0, 150.0, 100.0]);
    let overlay = e.overlay(1.0);
    assert_eq!(overlay.handles.len(), 4);
    assert_eq!(overlay.outlines, vec![[[-4.0, -4.0], [154.0, -4.0], [154.0, 104.0], [-4.0, 104.0]]]);
    assert_eq!(overlay.selection_box, None);
}

#[test]
fn a_selected_two_point_arrow_shows_its_points_and_drags_them() {
    let arrow = sample::with(
        sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [100.0, 0.0]]),
        json!({"roughness": 0, "roundness": null}),
    );
    let mut e = editor(vec![arrow]);
    click(&mut e, at(50.0, 0.0));
    let overlay = e.overlay(1.0);
    assert!(overlay.handles.is_empty() && overlay.outlines.is_empty());
    assert_eq!(overlay.points, vec![[0.0, 0.0], [100.0, 0.0]]);
    drag(&mut e, at(103.0, 2.0), [153.0, 52.0]);
    assert_eq!(element(&e, "a").to_value()["points"], json!([[0.0, 0.0], [150.0, 50.0]]));
    assert_eq!(selected(&e), ["a"]);
}

#[test]
fn select_all_then_delete_is_one_undo_step() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 10.0, 10.0]),
        sample::with(
            sample::generic("rectangle", "c", [50.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::text("t", [80.0, 40.0, 40.0, 20.0], "hi", Some("c")),
    ]);
    let before = e.file().as_ref().clone();
    assert!(!e.command(Command::Delete), "nothing selected");
    assert!(e.command(Command::SelectAll));
    assert_eq!(selected(&e), ["a", "c"]);
    assert!(e.command(Command::Delete));
    assert!(e.file().elements.iter().all(|el| el.is_deleted()));
    assert!(selected(&e).is_empty());
    assert!(e.command(Command::Undo));
    assert_eq!(*e.file().as_ref(), before);
    assert_eq!(selected(&e), ["a", "c"]);
}

#[test]
fn escape_clears_the_selection_then_has_nothing_to_cancel() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 10.0, 10.0])]);
    click(&mut e, at(5.0, 5.0));
    assert!(e.command(Command::Escape));
    assert!(selected(&e).is_empty());
    assert!(!e.command(Command::Escape));
}

#[test]
fn undo_waits_for_the_gesture_and_revision_counts_edits() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 10.0, 10.0])]);
    drag(&mut e, at(5.0, 5.0), [15.0, 5.0]);
    let after_first = e.revision();
    assert!(after_first > 0);
    e.pointer_down(at(15.0, 5.0));
    e.pointer_move(at(25.0, 5.0));
    assert!(!e.is_idle());
    assert!(!e.command(Command::Undo));
    e.pointer_up(at(25.0, 5.0));
    assert!(e.is_idle());
    assert!(e.command(Command::Undo));
    assert_eq!(rect_of(&e, "a")[..2], [10.0, 0.0]);
    assert!(e.revision() > after_first);
}

#[test]
fn a_drag_copies_the_scene_once() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 10.0, 10.0])]);
    drag(&mut e, at(5.0, 5.0), [45.0, 5.0]);
    assert_eq!(e.scene_clones(), 1, "only the first move after the undo snapshot copies");
}

#[test]
fn hand_tool_ignores_the_pointer_and_multi_selection_has_a_dashed_box() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 50.0]), solid("b", [200.0, 0.0, 50.0, 50.0])]);
    e.set_tool(Tool::Hand);
    drag(&mut e, at(50.0, 25.0), [80.0, 25.0]);
    assert_eq!(rect_of(&e, "a")[..2], [0.0, 0.0]);
    e.set_tool(Tool::Selection);
    e.command(Command::SelectAll);
    let overlay = e.overlay(1.0);
    assert_eq!(overlay.outlines.len(), 2);
    assert_eq!(overlay.selection_box, Some([-4.0, -4.0, 254.0, 54.0]));
    assert_eq!(overlay.handles.len(), 4);
}

#[test]
fn rotated_elements_move_but_have_no_handles() {
    let rotated = sample::with(solid("r", [0.0, 0.0, 100.0, 20.0]), json!({"angle": 0.5}));
    let mut e = editor(vec![rotated]);
    click(&mut e, at(50.0, 10.0));
    assert_eq!(selected(&e), ["r"]);
    assert!(e.overlay(1.0).handles.is_empty());
    drag(&mut e, at(50.0, 10.0), [60.0, 10.0]);
    assert_eq!(rect_of(&e, "r")[..2], [10.0, 0.0]);
}

#[test]
fn elbow_arrows_move_as_a_whole_without_point_or_resize_handles() {
    let elbow = sample::with(
        sample::linear("arrow", "e", [0.0, 0.0], &[[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]]),
        json!({"elbowed": true, "roughness": 0, "roundness": null}),
    );
    let mut e = editor(vec![elbow]);
    click(&mut e, at(50.0, 0.0));
    assert_eq!(selected(&e), ["e"]);
    let overlay = e.overlay(1.0);
    assert!(overlay.points.is_empty() && overlay.handles.is_empty());
    drag(&mut e, at(100.0, 50.0), [110.0, 60.0]);
    assert_eq!(rect_of(&e, "e")[..2], [10.0, 10.0]);
    assert_eq!(element(&e, "e").to_value()["points"], json!([[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]]));
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p scene --test editor_select`
Expected: 編譯失敗。

- [ ] **Step 4：實作**

讀：App.tsx 的 `handleCanvasPointerDown`（約第 8474 行）、`handleSelectionOnPointerDown`（約第 9415 行）、`onPointerMoveFromPointerDownHandler`（約第 10685 行，拖動與框選分支在第 10964、11483 行附近）、`onPointerUpFromPointerDownHandler`（點選收窄與 Shift 移除在第 12380 到 12615 行）、`maybeHandleResize`（約第 13833 行）、`clearSelection`、`clearSelectionIfNotUsingSelection`；`packages/element/src/linearElementEditor.ts` 的 `getPointIndexUnderCursor`（`POINT_HANDLE_SIZE`）；`packages/excalidraw/renderer/interactiveScene.ts` 的 `renderSelectionBorder` 與多選框（約第 1880 到 2070 行）；`packages/excalidraw/actions/actionDeleteSelected.tsx`、`actionSelectAll.ts`、`actionHistory.tsx`。

- `editor/mod.rs` 放公開型別、`Editor` 欄位、`command`、`overlay`、`set_tool`，以及修改場景的私有入口：`fn scene_mut(&mut self) -> &mut SceneFile`，在 `Arc::strong_count > 1` 時 `scene_clones += 1`，然後 `Arc::make_mut`。每次實際改了元件就 `revision += 1`。`editor/select.rs` 放選取工具的手勢。`Tool` 為建立工具時，Task 7 的指標事件什麼都不做（Task 8 補上）；`Hand` 永遠不處理指標事件（平移由 app 做）。
- 手勢狀態（私有 enum）：按下後依序判斷：(1) 單選一個 line 或非 elbow 的 arrow 時，點到它的某個點（距離 × zoom < 11）→ 拖點（elbow arrow 的端點不能編輯，spec §1.2）；(2) `transform::handle_at` → 縮放，記下 `transform::resize_offset`；(3) 其他照 `handleSelectionOnPointerDown`：`selection::element_at`、`hits_selection_box`、沒點到已選元件且沒按 Shift 且不在多選框內時立即清空選取，點到未選的元件就加入（Shift 疊加）並 `select_groups`。第一次移動（位置不同於按下點）時：按下時點到已選元件或多選框 → 拖動（`edit::drag_targets` 在此時計算一次）；否則 → 框選。放開時：沒有移動、點到元件、而且不是這次按下才加入的 → Shift 時移除（屬於群組就移除整個群組），否則選取收窄成該元件（群組展開）。沒有移動、只點到已選元件的外框而不是元件本身時清空選取（`hitElementBoundingBoxOnly`）。
- 拖動：`offset = pointer - origin`，Shift 時 `edit::lock_drag_axis`，再 `edit::apply_drag(file, start, targets, offset)`。縮放：`pointer - offset` 傳給 `transform::resize_element`（一個目標）或 `resize_elements`（多個目標，目標是選取的位置）。框選：每次移動 `selection::box_select`，Shift 時與按下時的選取合併。
- 手勢開始時保存 `(Arc<SceneFile> 的 clone, Selection)`，放開時 `history.record`。`command`：`Delete` 呼叫 `edit::delete_selection` 並立即記錄；`SelectAll` 是 `select_all`；`Undo`／`Redo` 還原選取、`revision += 1`；`Escape` 見 Task 8，Task 7 只處理閒置時清空選取。選取工具的手勢進行中，`Escape`、`Undo`、`Redo`、`Delete`、`SelectAll` 都回傳 `false` 且不做事。
- `cursor`：`pointer_move` 閒置時更新：在控制點上是對應的 resize 游標（`Nw`／`Se` → `ResizeNwse`，`Ne`／`Sw` → `ResizeNesw`，`N`／`S` → `ResizeNs`，`E`／`W` → `ResizeEw`），在單選 line／arrow 的點上是 `Pointer`，點得到元件或在多選框內是 `Move`，其他 `Default`；建立工具一律 `Crosshair`。
- `overlay`：outline 用 `GeometryCache::absolute_coords` 的框加 4 / zoom、依元件 `angle` 繞中心旋轉四角，角的順序是左上、右上、右下、左下；單選兩點 line／arrow 不畫 outline。
- 刪除的元件不能留在選取裡：每次改動場景後把不存在或已刪除的 id 從選取移除。

- [ ] **Step 5：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 6：Commit**

```bash
git add crates/scene
git commit -m "Add the scene editor state machine with the selection tool"
```

---

### Task 8：建立工具與 undo property test

**Files:**
- Create: `crates/scene/src/editor/create.rs`、`crates/scene/src/editor/style.rs`、`crates/scene/tests/editor_create.rs`、`crates/scene/tests/undo_property.rs`
- Modify: `crates/scene/src/editor/mod.rs`

**Interfaces:**
- Consumes: Task 7 `Editor`；M2 `new_element::{new_generic_element, new_line_element, new_arrow_element, new_freedraw_element, ElementProps, GenericKind}`；Task 5 `edit::append_element`、`lock_linear_angle`；Task 2 `is_path_a_loop`、`LINE_CONFIRM_THRESHOLD`。
- Produces:

```rust
// editor/style.rs, re-exported from editor
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeWidth { Thin, #[default] Medium, Bold, ExtraBold }
impl StrokeWidth {
    /// `STROKE_WIDTH` (1, 2, 4, 8), or `FREEDRAW_STROKE_WIDTH` (0.5, 1, 2, 4) for freedraw.
    pub fn value(self, freedraw: bool) -> f64;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeStyle { Sharp, #[default] Round }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrowType { Sharp, #[default] Round }

/// The appState `currentItem*` values new elements take (`getDefaultAppState`).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemStyle {
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: StrokeWidth,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub edges: EdgeStyle,
    pub arrow_type: ArrowType,
    pub start_arrowhead: Option<String>,
    pub end_arrowhead: Option<String>,
    pub stroke_variability: String,
}
impl Default for ItemStyle { /* "#1e1e1e", "transparent", "solid", Medium, "solid", 1, 100, Round, Round, None, Some("arrow"), "constant" */ }

impl<E: Env> Editor<E> {
    pub fn style(&self) -> &ItemStyle;
}
```

- [ ] **Step 1：`crates/scene/tests/editor_create.rs`**

```rust
mod support;

use scene::editor::{Command, Editor, Tool};
use serde_json::{Value, json};
use support::*;

fn live(e: &Editor<TestEnv>) -> Vec<Value> {
    e.file().elements.iter().filter(|el| !el.is_deleted()).map(|el| el.to_value()).collect()
}

fn only(e: &Editor<TestEnv>) -> Value {
    let live = live(e);
    assert_eq!(live.len(), 1, "{live:?}");
    live[0].clone()
}

fn last(e: &Editor<TestEnv>) -> Value {
    e.file().elements.last().expect("an element").to_value()
}

fn xywh(v: &Value) -> [f64; 4] {
    ["x", "y", "width", "height"].map(|k| v[k].as_f64().expect(k))
}

#[test]
fn rectangle_drag_creates_a_selected_element_and_returns_to_selection() {
    let mut e = editor(vec![]);
    assert_eq!(e.style().stroke_width.value(false), 2.0);
    e.set_tool(Tool::Rectangle);
    drag(&mut e, at(10.0, 20.0), [110.0, 70.0]);
    assert_eq!(e.tool(), Tool::Selection);
    let v = only(&e);
    assert_eq!(v["type"], "rectangle");
    assert_eq!(xywh(&v), [10.0, 20.0, 100.0, 50.0]);
    assert_eq!(v["roundness"], json!({"type": 3}));
    assert_eq!(v["strokeWidth"], json!(2.0));
    assert_eq!(v["strokeColor"], "#1e1e1e");
    assert_eq!(v["backgroundColor"], "transparent");
    assert!(v["index"].is_string());
    assert_eq!(v["id"].as_str().map(str::len), Some(21));
    assert_eq!(selected(&e), [v["id"].as_str().unwrap()]);
}

#[test]
fn shift_makes_squares_alt_centers_and_up_left_drags_flip() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    drag(&mut e, shift(10.0, 10.0), [60.0, 30.0]);
    assert_eq!(xywh(&last(&e)), [10.0, 10.0, 50.0, 50.0]);

    e.set_tool(Tool::Ellipse);
    drag(&mut e, alt(100.0, 100.0), [120.0, 110.0]);
    assert_eq!(xywh(&last(&e)), [80.0, 90.0, 40.0, 20.0]);
    assert_eq!(last(&e)["roundness"], json!({"type": 2}));

    e.set_tool(Tool::Diamond);
    drag(&mut e, at(100.0, 100.0), [60.0, 70.0]);
    assert_eq!(xywh(&last(&e)), [60.0, 70.0, 40.0, 30.0]);
    assert_eq!(last(&e)["type"], "diamond");
}

#[test]
fn a_click_without_a_drag_creates_nothing() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    click(&mut e, at(10.0, 10.0));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));
}

#[test]
fn arrow_drag_creates_a_two_point_arrow_anchored_at_the_press() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Arrow);
    drag(&mut e, at(100.0, 100.0), [40.0, 160.0]);
    let v = only(&e);
    assert_eq!(v["type"], "arrow");
    assert_eq!(xywh(&v), [100.0, 100.0, 60.0, 60.0]);
    assert_eq!(v["points"], json!([[0.0, 0.0], [-60.0, 60.0]]));
    assert_eq!(v["endArrowhead"], "arrow");
    assert_eq!(v["startArrowhead"], Value::Null);
    assert_eq!(v["roundness"], json!({"type": 2}));
    assert_eq!(v["elbowed"], json!(false));
    assert_eq!(e.tool(), Tool::Selection);
    assert_eq!(selected(&e), [v["id"].as_str().unwrap()]);
}

#[test]
fn clicks_build_a_multi_point_line_that_enter_finishes() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    assert!(!e.is_idle());
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 100.0));
    click(&mut e, at(100.0, 100.0));
    e.pointer_move(at(50.0, 150.0));
    assert!(e.command(Command::Finalize));
    assert!(e.is_idle());
    let v = only(&e);
    assert_eq!(v["points"], json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]));
    assert_eq!(v["polygon"], json!(false));
    assert_eq!(e.tool(), Tool::Selection);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty(), "the whole line is one undo step");
}

#[test]
fn clicking_back_on_the_start_closes_a_line_into_a_polygon() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 100.0));
    click(&mut e, at(100.0, 100.0));
    e.pointer_move(at(2.0, 2.0));
    click(&mut e, at(2.0, 2.0));
    assert!(e.is_idle());
    let v = only(&e);
    assert_eq!(v["points"], json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 0.0]]));
    assert_eq!(v["polygon"], json!(true));
}

#[test]
fn freedraw_records_relative_points_and_keeps_the_tool() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Freedraw);
    e.pointer_down(at(10.0, 10.0));
    e.pointer_move(at(15.0, 12.0));
    e.pointer_move(at(15.0, 12.0));
    e.pointer_move(at(5.0, 20.0));
    e.pointer_up(at(8.0, 25.0));
    let v = only(&e);
    assert_eq!(v["points"], json!([[0.0, 0.0], [5.0, 2.0], [-5.0, 10.0], [-2.0, 15.0]]));
    assert_eq!(xywh(&v), [10.0, 10.0, 10.0, 15.0]);
    assert_eq!(v["simulatePressure"], json!(true));
    assert_eq!(v["pressures"], json!([]));
    assert_eq!(v["strokeWidth"], json!(1.0));
    assert_eq!(v["strokeOptions"], json!({"variability": "constant", "streamline": 0.5}));
    assert_eq!(v["roundness"], Value::Null);
    assert_eq!(e.tool(), Tool::Freedraw);
    assert!(selected(&e).is_empty());

    click(&mut e, at(50.0, 50.0));
    assert_eq!(last(&e)["points"], json!([[0.0, 0.0], [0.0001, 0.0001]]));
}

#[test]
fn escape_discards_a_dragged_shape_but_finishes_a_multi_point_arrow() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    e.pointer_down(at(0.0, 0.0));
    e.pointer_move(at(50.0, 50.0));
    assert!(e.command(Command::Escape));
    e.pointer_up(at(50.0, 50.0));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));

    e.set_tool(Tool::Arrow);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 80.0));
    assert!(e.command(Command::Escape));
    assert_eq!(only(&e)["points"], json!([[0.0, 0.0], [100.0, 0.0]]));
}

#[test]
fn switching_tools_finishes_a_multi_point_line() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(60.0, 0.0));
    click(&mut e, at(60.0, 0.0));
    e.pointer_move(at(60.0, 60.0));
    e.set_tool(Tool::Rectangle);
    assert!(e.is_idle());
    assert_eq!(e.tool(), Tool::Rectangle);
    assert_eq!(only(&e)["points"], json!([[0.0, 0.0], [60.0, 0.0]]));
    assert!(selected(&e).is_empty());
}

#[test]
fn a_tiny_arrow_is_removed_and_creation_undoes_in_one_step() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Arrow);
    click(&mut e, at(10.0, 10.0));
    assert!(e.command(Command::Finalize));
    assert!(e.file().elements.is_empty());

    e.set_tool(Tool::Ellipse);
    drag(&mut e, at(0.0, 0.0), [30.0, 30.0]);
    let created = e.file().as_ref().clone();
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
    assert!(e.command(Command::Redo));
    assert_eq!(*e.file().as_ref(), created);
}
```

- [ ] **Step 2：`crates/scene/tests/undo_property.rs`**（spec §9.2）

```rust
mod support;

use scene::editor::{Command, Editor, Tool};
use scene::sample;
use serde_json::{Value, json};
use support::*;

/// Numerical Recipes LCG picking operations and coordinates.
struct Ops(u32);

impl Ops {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    fn coord(&mut self) -> f64 {
        f64::from(self.below(60)) * 10.0
    }
}

fn starting_scene() -> Vec<Value> {
    vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 80.0]),
            json!({"backgroundColor": "#ffc9c9", "boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}]}),
        ),
        sample::with(
            sample::text("t", [20.0, 30.0, 60.0, 20.0], "hi", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
        sample::with(
            sample::linear("arrow", "a", [150.0, 40.0], &[[0.0, 0.0], [120.0, 60.0]]),
            json!({"startBinding": {"elementId": "r", "fixedPoint": [1.0, 0.5], "mode": "orbit"}}),
        ),
        sample::with(sample::generic("ellipse", "e", [300.0, 200.0, 80.0, 80.0]), json!({"groupIds": ["g"]})),
        sample::with(sample::generic("diamond", "d", [400.0, 200.0, 80.0, 80.0]), json!({"groupIds": ["g"]})),
        sample::freedraw("f", [50.0, 300.0], &[[0.0, 0.0], [10.0, 5.0], [20.0, -3.0]]),
        sample::linear("line", "l", [200.0, 400.0], &[[0.0, 0.0], [50.0, 50.0], [100.0, 0.0]]),
        json!({"id": "img", "type": "image", "x": 500, "y": 50, "width": 60, "height": 40, "angle": 0,
               "isDeleted": false, "version": 1, "versionNonce": 1, "fileId": "file-1"}),
    ]
}

fn center(b: [f64; 4]) -> (f64, f64) {
    ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
}

fn random_step(e: &mut Editor<TestEnv>, ops: &mut Ops) {
    let (x, y) = (ops.coord(), ops.coord());
    let to = [ops.coord(), ops.coord()];
    match ops.below(14) {
        0 => {
            e.set_tool(Tool::Rectangle);
            drag(e, at(x, y), to);
        }
        1 => {
            e.set_tool(Tool::Ellipse);
            drag(e, alt(x, y), to);
        }
        2 => {
            e.set_tool(Tool::Arrow);
            drag(e, at(x, y), to);
        }
        3 => {
            e.set_tool(Tool::Line);
            click(e, at(x, y));
            e.pointer_move(at(to[0], to[1]));
            click(e, at(to[0], to[1]));
            e.command(Command::Finalize);
        }
        4 => {
            e.set_tool(Tool::Freedraw);
            drag(e, at(x, y), to);
            e.set_tool(Tool::Selection);
        }
        5 => {
            e.set_tool(Tool::Selection);
            click(e, at(x, y));
        }
        6 => {
            e.set_tool(Tool::Selection);
            drag(e, at(x, y), to);
        }
        7 => {
            e.set_tool(Tool::Selection);
            drag(e, shift(x, y), to);
        }
        8 => {
            e.command(Command::SelectAll);
        }
        9 => {
            e.command(Command::Delete);
        }
        10 => {
            e.command(Command::Undo);
        }
        11 => {
            e.command(Command::Redo);
        }
        12 => {
            if let Some(handle) = e.overlay(1.0).handles.first().copied() {
                let (hx, hy) = center(handle);
                drag(e, at(hx, hy), to);
            }
        }
        _ => {
            if let Some(point) = e.overlay(1.0).points.last().copied() {
                drag(e, at(point[0], point[1]), to);
            }
        }
    }
}

#[test]
fn undoing_everything_restores_the_initial_scene() {
    for seed in 1..=150u32 {
        let mut e = Editor::new(sample::file(starting_scene()), TestEnv::seeded(u64::from(seed)));
        let initial = e.file().as_ref().clone();
        let mut ops = Ops(seed);
        for _ in 0..60 {
            random_step(&mut e, &mut ops);
        }
        e.command(Command::Finalize);
        e.set_tool(Tool::Selection);
        let finished = e.file().as_ref().clone();
        let mut undone = 0;
        while e.command(Command::Undo) {
            undone += 1;
        }
        assert_eq!(*e.file().as_ref(), initial, "seed {seed}: undo everything");
        for _ in 0..undone {
            assert!(e.command(Command::Redo), "seed {seed}");
        }
        assert_eq!(*e.file().as_ref(), finished, "seed {seed}: redo everything");
    }
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p scene --test editor_create --test undo_property`
Expected: 編譯失敗（`style`、建立工具不存在）。

- [ ] **Step 4：實作**

讀：`packages/excalidraw/appState.ts` 的 `getDefaultAppState`；`packages/common/src/constants.ts` 的 `STROKE_WIDTH`、`FREEDRAW_STROKE_WIDTH`、`ROUNDNESS`、`MINIMUM_ARROW_SIZE`、`LINE_CONFIRM_THRESHOLD`、`DEFAULT_STROKE_STREAMLINE`；`packages/element/src/typeChecks.ts` 的 `isUsingAdaptiveRadius`；App.tsx 的 `getCurrentItemRoundness`（約第 10485 行）、`createGenericElementOnPointerDown`（約第 10511 行）、`handleLinearElementOnPointerDown`（約第 10189 到 10482 行）、`handleFreeDrawElementOnPointerDown`（約第 9966 行）、`handleCanvasPointerMove` 的多點線段分支（約第 7934 到 8046 行）、自由筆畫的移動（約第 11394 行）與放開（約第 11885 行）、線段放開（約第 11915 到 12002 行）、`maybeDragNewGenericElement`（約第 13600 行）；`packages/element/src/dragElements.ts` 的 `dragNewElement`；`packages/element/src/sizeHelpers.ts` 的 `getPerfectElementSize`、`isInvisiblySmallElement`；`packages/excalidraw/actions/actionFinalize.tsx`。

- 新元件的 props 從 `ItemStyle` 來：rectangle 的圓角 `Round` 時 `{type: 3}`，diamond、ellipse、line `Round` 時 `{type: 2}`，`Sharp` 時 `null`；arrow 看 `arrow_type`；freedraw 一律 `null`，線寬用 `value(true)`。`seed`、`id`、`versionNonce` 等由 M2 的建構函數與 `Env` 產生，建立後 `edit::append_element`。
- 形狀（rectangle／diamond／ellipse）：按下時建立 0×0 元件並加入場景；移動照 `dragNewElement`（Shift、Alt、往左上拖）且只在寬高都不為 0 時更新；放開時寬高都是 0 就把它從 `elements` 移除，否則選取它、工具回到 Selection。
- 線與箭頭：按下時建立 `points: [[0,0],[0,0]]`；拖動時最後一點跟著指標（Shift 用 `lock_linear_angle` 以點 0 為原點），寬高用 `size_from_points`，`x`、`y` 不動。放開時沒有移動或拖動距離 × zoom 小於 `MINIMUM_ARROW_SIZE`（20）→ 進入多點模式，已確認的點只有點 0；否則完成。
- 多點模式（私有狀態記「已確認的點數」）：移動時沒有未確認點且離最後確認點 ≥ `LINE_CONFIRM_THRESHOLD` → 加一個跟隨點；有跟隨點、點數大於 2、離最後確認點 < 8 → 移除跟隨點；有跟隨點 → 更新它（Shift 以前一點為原點鎖角度）。按下時：line 且 `is_path_a_loop(points, zoom)` → 確認跟隨點後完成；點數大於 1 且按下點離最後確認點 < 8 → 完成；其他什麼都不做。放開時把跟隨點變成確認點。
- 完成（`Finalize`、`Escape`、切換工具、上面兩種點擊）：丟掉未確認的跟隨點；`isInvisiblySmallElement`（點數小於 2，或兩點的 arrow 兩端距離 ≤ 0.1）時把元件移除；line 與 freedraw 若 `is_path_a_loop` 則最後一點設成第一點，line 的 `polygon` 設成「點數大於 3 且首尾相同」（`polygon` 在 `extra` 裡，只改 line）；選取它（freedraw 除外）；工具回到 Selection（freedraw 保持）。
- freedraw：按下時 `points: [[0,0]]`、`simulatePressure: true`、`pressures: []`、`strokeOptions: {variability, streamline: 0.5}`；移動時加上相對 `x`、`y` 的點（與最後一點相同就跳過）；放開時加上放開位置，若等於點 0 則兩軸加 0.0001；然後完成。
- 一次建立（從第一次按下到完成）是一個 undo 步驟。建立中途被移除的元件不留下歷史。
- `Escape`：拖曳中的形狀、線段或筆畫 → 移除並結束手勢（之後的 `pointer_up` 不做事），回傳 `true`；多點模式 → 完成，回傳 `true`；否則照 Task 7。

- [ ] **Step 5：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過；`undo_property` 在 release 以外的建置也要在 60 秒內跑完，超過時把種子數降到 60 並在測試旁註明原因。

- [ ] **Step 6：Commit**

```bash
git add crates/scene
git commit -m "Add shape, line, arrow and freedraw tools with an undo property test"
```

---

### Task 9：存放位置、原子寫入與自動存檔邏輯

**Files:**
- Create: `crates/app/src/storage.rs`、`crates/app/src/autosave.rs`、`crates/app/src/writer.rs`
- Modify: `crates/scene/src/file.rs`、`crates/app/src/lib.rs`

**Interfaces:**
- Consumes: M2 `SceneFile`、`NapkinView`。
- Produces:

```rust
// scene/src/file.rs
impl SceneFile {
    /// Like `to_json_string`, with `appState.napkin` set to `view` in the written text only.
    pub fn to_json_string_with_view(&self, view: Option<NapkinView>) -> String;
}
// `from_json_str` also accepts `"elements": null` and `"appState": null` as empty.

// app/src/storage.rs
pub struct Paths { pub canvases: PathBuf, pub last: PathBuf }
impl Paths {
    /// `<home>/Documents/napkin` and `<home>/.local/state/napkin/last` (spec §5.4).
    pub fn from_home(home: &Path) -> Paths;
    /// From `$HOME`; `None` when it is unset or empty.
    pub fn from_env() -> Option<Paths>;
    pub fn scratch(&self) -> PathBuf;
}

#[derive(Debug)]
pub enum Loaded {
    Missing,
    Parsed { file: scene::SceneFile, mtime: SystemTime },
    /// Unreadable or unparseable; the message names the path.
    Invalid(String),
}
pub fn load(path: &Path) -> Loaded;

#[derive(Debug)]
pub enum Content {
    /// `mtime` is `None` when the file does not exist yet.
    Editable { file: scene::SceneFile, mtime: Option<SystemTime> },
    /// Shown, never written (spec §8).
    Unreadable(String),
}

#[derive(Debug)]
pub struct Opened {
    pub path: PathBuf,
    /// The file stem.
    pub name: String,
    pub content: Content,
    /// Why another file than the remembered one opened (spec §8).
    pub notice: Option<String>,
}
/// Spec §5.4 and §8: the requested file (even if missing or unparseable), else the file in
/// `last` if it parses, else `scratch`, created when missing. Writes nothing else.
pub fn open_at_startup(paths: &Paths, requested: Option<&Path>) -> Opened;
/// Stores `path` (made absolute) in `last`, creating its directory.
pub fn remember(paths: &Paths, path: &Path) -> io::Result<()>;
/// Writes a sibling temporary file, syncs it, renames it over `path` and syncs the directory
/// (spec §5.5). Creates missing parent directories; removes the temporary file on failure.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<SystemTime>;
/// `None` when the file does not exist.
pub fn modified(path: &Path) -> io::Result<Option<SystemTime>>;

// app/src/autosave.rs
pub const DEBOUNCE: Duration = Duration::from_millis(500);
pub const RETRY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger { Frame, FocusLost, Exit }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DocumentState { pub revision: u64, pub view: NapkinView, pub idle: bool }

#[derive(Debug)]
pub struct Autosave { /* saved revision and view, last seen revision, changed_at, in_flight, error, failed_at */ }
impl Autosave {
    /// Everything counts as saved.
    pub fn new(revision: u64, view: NapkinView) -> Autosave;
    /// Call every frame with `Trigger::Frame`, and once on focus loss or exit (spec §5.5).
    pub fn should_save(&mut self, now: Instant, state: DocumentState, trigger: Trigger) -> bool;
    pub fn started(&mut self, state: DocumentState);
    pub fn finished(&mut self, now: Instant, result: Result<(), String>);
    pub fn in_flight(&self) -> bool;
    pub fn error(&self) -> Option<&str>;
    /// Element changes not yet written (a failed or running save counts as not written).
    pub fn has_unsaved_changes(&self, revision: u64) -> bool;
    /// How long until `should_save(Trigger::Frame)` can turn true without new input.
    pub fn wake_after(&self, now: Instant) -> Option<Duration>;
    /// After a reload: everything counts as saved, errors cleared.
    pub fn reset(&mut self, revision: u64, view: NapkinView);
}

// app/src/writer.rs
pub struct SaveJob { pub path: PathBuf, pub file: Arc<scene::SceneFile>, pub view: NapkinView }
pub type SaveResult = Result<SystemTime, String>;
/// Serializes with the view and writes atomically.
pub fn save(job: &SaveJob) -> SaveResult;
pub struct SaveWorker { /* job sender, result receiver, thread */ }
impl SaveWorker {
    /// `wake` runs on the worker thread after every job (the app requests a repaint).
    pub fn spawn(wake: impl Fn() + Send + 'static) -> SaveWorker;
    pub fn submit(&self, job: SaveJob);
    pub fn try_result(&self) -> Option<SaveResult>;
    /// Finishes queued jobs, joins the thread and returns results not yet taken.
    pub fn shutdown(self) -> Vec<SaveResult>;
}
```

- [ ] **Step 1：`file.rs` 的測試**

```rust
    #[test]
    fn null_elements_and_app_state_load_as_empty() {
        let file = SceneFile::from_json_str(r#"{"type":"excalidraw","elements":null,"appState":null}"#).unwrap();
        assert!(file.elements.is_empty() && file.app_state.is_empty());
        let written: Value = serde_json::from_str(&file.to_json_string()).unwrap();
        assert!(semantic_eq(&written, &json!({"type": "excalidraw", "elements": [], "appState": {}})), "{written}");
        assert!(matches!(
            SceneFile::from_json_str(r#"{"type":"excalidraw","appState":3}"#),
            Err(LoadError::AppStateNotObject)
        ));
    }

    #[test]
    fn writing_with_a_view_leaves_the_file_untouched() {
        let file = SceneFile::new();
        let view = NapkinView { scroll_x: 1.0, scroll_y: 2.0, zoom: 1.5 };
        let text = file.to_json_string_with_view(Some(view));
        assert_eq!(SceneFile::from_json_str(&text).unwrap().napkin_view(), Some(view));
        assert_eq!(file.napkin_view(), None);
        assert_eq!(file.to_json_string_with_view(None), file.to_json_string());
    }
```

- [ ] **Step 2：`storage.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(name: &str) -> TempHome {
            let dir = std::env::temp_dir().join(format!("napkin-storage-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempHome(dir)
        }

        fn paths(&self) -> Paths {
            Paths::from_home(&self.0)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const VALID: &str = r#"{"type":"excalidraw","version":2,"elements":[],"appState":{}}"#;

    #[test]
    fn paths_follow_the_spec() {
        let paths = Paths::from_home(Path::new("/home/u"));
        assert_eq!(paths.canvases, PathBuf::from("/home/u/Documents/napkin"));
        assert_eq!(paths.last, PathBuf::from("/home/u/.local/state/napkin/last"));
        assert_eq!(paths.scratch(), PathBuf::from("/home/u/Documents/napkin/scratch.excalidraw"));
    }

    #[test]
    fn first_start_creates_scratch() {
        let home = TempHome::new("first");
        let paths = home.paths();
        let opened = open_at_startup(&paths, None);
        assert_eq!(opened.path, paths.scratch());
        assert_eq!(opened.name, "scratch");
        assert!(matches!(opened.content, Content::Editable { mtime: Some(_), .. }));
        assert!(opened.notice.is_none());
        assert!(paths.scratch().exists());
    }

    #[test]
    fn reopens_the_last_file_and_falls_back_when_it_is_broken() {
        let home = TempHome::new("last");
        let paths = home.paths();
        let good = home.0.join("good.excalidraw");
        std::fs::write(&good, VALID).unwrap();
        remember(&paths, &good).unwrap();
        let opened = open_at_startup(&paths, None);
        assert_eq!((opened.path.as_path(), opened.name.as_str()), (good.as_path(), "good"));

        std::fs::write(&good, "{ broken").unwrap();
        let opened = open_at_startup(&paths, None);
        assert_eq!(opened.path, paths.scratch());
        let notice = opened.notice.expect("a notice about the broken file");
        assert!(notice.contains("good.excalidraw"), "{notice}");
        assert_eq!(std::fs::read_to_string(&good).unwrap(), "{ broken", "never overwritten");

        std::fs::remove_file(&good).unwrap();
        let opened = open_at_startup(&paths, None);
        assert_eq!(opened.path, paths.scratch());
        assert!(opened.notice.is_none(), "a vanished file is not an error");
    }

    #[test]
    fn requested_files_open_even_when_missing_or_broken() {
        let home = TempHome::new("requested");
        let paths = home.paths();
        let missing = home.0.join("new.excalidraw");
        let opened = open_at_startup(&paths, Some(&missing));
        assert!(matches!(opened.content, Content::Editable { mtime: None, .. }));
        assert!(!missing.exists(), "created on the first save");

        let broken = home.0.join("broken.excalidraw");
        std::fs::write(&broken, "[]").unwrap();
        let opened = open_at_startup(&paths, Some(&broken));
        assert!(matches!(&opened.content, Content::Unreadable(e) if e.contains("broken.excalidraw")));
    }

    #[test]
    fn atomic_writes_replace_contents_and_report_the_mtime() {
        let home = TempHome::new("atomic");
        let path = home.0.join("nested/dir/a.excalidraw");
        let mtime = write_atomic(&path, VALID).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
        assert_eq!(modified(&path).unwrap(), Some(mtime));
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1, "no temporary file left");
        assert_eq!(modified(&home.0.join("absent")).unwrap(), None);
        assert!(matches!(load(&home.0.join("absent")), Loaded::Missing));
    }
}
```

- [ ] **Step 3：`autosave.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> NapkinView {
        NapkinView { scroll_x: 0.0, scroll_y: 0.0, zoom: 1.0 }
    }

    fn state(revision: u64) -> DocumentState {
        DocumentState { revision, view: view(), idle: true }
    }

    #[test]
    fn saves_500ms_after_the_last_change_when_idle() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut a = Autosave::new(0, view());
        assert!(!a.should_save(t0, state(0), Trigger::Frame));
        assert!(!a.should_save(t0, state(1), Trigger::Frame));
        assert_eq!(a.wake_after(t0), Some(DEBOUNCE));
        assert!(!a.should_save(ms(300), state(2), Trigger::Frame), "a new change restarts the wait");
        assert!(!a.should_save(ms(700), DocumentState { idle: false, ..state(2) }, Trigger::Frame));
        assert!(!a.should_save(ms(799), state(2), Trigger::Frame));
        assert!(a.should_save(ms(800), state(2), Trigger::Frame));
        a.started(state(2));
        assert!(a.in_flight() && a.has_unsaved_changes(2));
        assert!(!a.should_save(ms(900), state(2), Trigger::Frame), "one write at a time");
        a.finished(ms(950), Ok(()));
        assert!(!a.has_unsaved_changes(2));
        assert!(!a.should_save(ms(2000), state(2), Trigger::Frame));
        assert_eq!(a.wake_after(ms(2000)), None);
    }

    #[test]
    fn focus_loss_and_exit_write_view_changes_at_once() {
        let t0 = Instant::now();
        let mut a = Autosave::new(0, view());
        let panned = DocumentState { view: NapkinView { scroll_x: 5.0, ..view() }, ..state(0) };
        assert!(!a.should_save(t0, panned, Trigger::Frame), "panning alone waits for focus loss");
        assert!(!a.has_unsaved_changes(0));
        assert!(a.should_save(t0, panned, Trigger::FocusLost));
        a.started(panned);
        a.finished(t0, Ok(()));
        assert!(!a.should_save(t0, panned, Trigger::Exit));
        assert!(a.should_save(t0, DocumentState { idle: false, ..state(3) }, Trigger::Exit));
    }

    #[test]
    fn failures_stay_reported_and_retry_every_five_seconds() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut a = Autosave::new(0, view());
        a.should_save(t0, state(1), Trigger::Frame);
        assert!(a.should_save(ms(500), state(1), Trigger::Frame));
        a.started(state(1));
        a.finished(ms(510), Err("disk full".into()));
        assert_eq!(a.error(), Some("disk full"));
        assert!(a.has_unsaved_changes(1));
        assert!(!a.should_save(ms(5000), state(1), Trigger::Frame));
        assert_eq!(a.wake_after(ms(5000)), Some(Duration::from_millis(510)));
        assert!(a.should_save(ms(5510), state(1), Trigger::Frame));
        a.started(state(1));
        a.finished(ms(5520), Ok(()));
        assert_eq!(a.error(), None);
    }

    #[test]
    fn reset_after_a_reload_counts_as_saved() {
        let t0 = Instant::now();
        let mut a = Autosave::new(0, view());
        a.should_save(t0, state(4), Trigger::Frame);
        a.reset(7, view());
        assert!(!a.has_unsaved_changes(7));
        assert!(!a.should_save(t0 + DEBOUNCE, state(7), Trigger::FocusLost));
    }
}
```

- [ ] **Step 4：`writer.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn saves_in_the_background_and_reports_back() {
        let dir = std::env::temp_dir().join(format!("napkin-writer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (woken, wake) = mpsc::channel();
        let worker = SaveWorker::spawn(move || {
            let _ = woken.send(());
        });
        let view = NapkinView { scroll_x: 3.0, scroll_y: 4.0, zoom: 2.0 };
        let path = dir.join("w.excalidraw");
        worker.submit(SaveJob { path: path.clone(), file: Arc::new(SceneFile::new()), view });
        wake.recv_timeout(Duration::from_secs(10)).expect("the worker woke the UI");
        let mtime = worker.try_result().expect("a result").expect("saved");
        let text = std::fs::read_to_string(&path).expect("file written");
        assert_eq!(SceneFile::from_json_str(&text).expect("valid").napkin_view(), Some(view));
        assert_eq!(storage::modified(&path).expect("stat"), Some(mtime));

        // The path is a non-empty directory, so the final rename fails.
        worker.submit(SaveJob { path: dir.clone(), file: Arc::new(SceneFile::new()), view });
        let results = worker.shutdown();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err(), "{results:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 5：確認失敗**

Run: `cargo test -p scene --lib file` 與 `cargo test -p app --lib storage autosave writer`
Expected: 編譯失敗。

- [ ] **Step 6：實作**

- `file.rs`：`from_json_str` 的 `elements`、`appState` 遇到 `Value::Null` 時當成空的；`to_json_string` 改成呼叫 `to_json_string_with_view(None)`；帶視角時複製 `app_state` 插入 `"napkin"`（格式同 `set_napkin_view`）。
- `storage.rs`：`load` 的錯誤訊息格式 `"{path}: {error}"`（`NotFound` 是 `Missing`，其他 I/O 錯誤與解析錯誤是 `Invalid`）。`open_at_startup` 照決定 8：有 `requested` → `load` 它，`Missing` 時 `Editable { file: SceneFile::new(), mtime: None }`，`Invalid` 時 `Unreadable`；沒有 → 讀 `last`（讀不到或空白當成沒有），指向的檔案 `Parsed` 就開它，`Missing` 就走 scratch 且沒有 notice，`Invalid` 就走 scratch 並帶 notice（含原因）；scratch `Missing` 時用 `write_atomic` 寫入 `SceneFile::new().to_json_string()`，寫入失敗或 scratch `Invalid` 時 `Unreadable`。`name` 是 `file_stem`。
- `write_atomic`：決定 12。暫存檔名 `.<file name>.napkin-tmp`，放在同一個目錄；失敗時刪除暫存檔。回傳 rename 之後讀到的 `modified()`。
- `autosave.rs`：每次 `should_save` 看到與上次不同的 `revision` 就把 `changed_at` 設成 `now`。`Frame`：元件有變動、`idle`、距 `changed_at` 至少 `DEBOUNCE`、沒有寫入中、沒有錯誤或距上次失敗至少 `RETRY`。`FocusLost`／`Exit`：元件或視角與上次成功寫入的不同、沒有寫入中。`finished(Ok)` 把 `started` 的狀態記為已存。
- `writer.rs`：一條執行緒迴圈接 `SaveJob`，`save` 之後把結果送回並呼叫 `wake`。`save` 用 `job.file.to_json_string_with_view(Some(job.view))` 與 `storage::write_atomic`，錯誤訊息含路徑。

- [ ] **Step 7：驗證**

Run: `cargo test --workspace` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 8：Commit**

```bash
git add crates/scene crates/app
git commit -m "Add canvas storage paths, atomic writes and the autosave schedule"
```

---

### Task 10：畫布編輯整合

**Files:**
- Create: `crates/app/src/edit_input.rs`、`crates/app/src/overlay.rs`
- Move: `crates/app/src/viewer.rs` → `crates/app/src/napkin_app.rs`（`git mv`；`Viewer` → `NapkinApp`）
- Modify: `crates/app/src/lib.rs`、`crates/app/src/main.rs`、`crates/app/src/input.rs`、`crates/app/src/render/cache.rs`、`crates/app/src/render/plan.rs`、`crates/app/src/render/gpu.rs`、`crates/app/tests/support/mod.rs`、`crates/app/tests/gpu_shapes.rs`、`crates/app/tests/gpu_text.rs`

**Interfaces:**
- Consumes: Task 7、8 的 `Editor`、`Tool`、`Command`、`Cursor`、`Overlay`、`PointerEvent`、`Modifiers`；M3 的 `Camera`、`CanvasInput`、`CanvasFrame`、`SceneCache`、`Theme`。
- Produces:

```rust
// edit_input.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditorInput {
    Down(PointerEvent),
    Move(PointerEvent),
    Up(PointerEvent),
    Tool(Tool),
    Command(Command),
}

/// A primary press that started on the canvas, and the last pointer position in scene
/// coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerCapture { /* pressed: bool, last: Option<[f64; 2]> */ }

pub struct FrameInput<'a> {
    pub events: &'a [egui::Event],
    /// The canvas rectangle in egui points.
    pub canvas: egui::Rect,
    pub camera: Camera,
    /// `egui::InputState::modifiers` for events that carry none.
    pub modifiers: egui::Modifiers,
    /// Space held or the Hand tool active: primary presses pan instead of editing.
    pub panning: bool,
    /// An egui widget has keyboard focus (`Context::wants_keyboard_input`).
    pub keyboard_taken: bool,
    pub focused: bool,
}

/// This frame's editor inputs in event order.
pub fn translate(input: &FrameInput, capture: &mut PointerCapture) -> Vec<EditorInput>;
pub fn cursor_icon(cursor: Cursor) -> egui::CursorIcon;

// overlay.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayColors { pub accent: egui::Color32, pub selection: egui::Color32, pub handle_fill: egui::Color32 }
impl OverlayColors { pub fn from_theme(theme: &Theme) -> OverlayColors; }
/// `overlay` in screen points for a canvas whose top-left corner is `origin`.
pub fn shapes(overlay: &Overlay, camera: &Camera, origin: egui::Pos2, colors: OverlayColors) -> Vec<egui::Shape>;

// render/cache.rs: MeshKey (and the private ShapeKey) gain
pub version_nonce_bits: u64,
// render/gpu.rs: CanvasFrame gains
/// Changes when the scene is replaced (a reload); the renderer then drops every cache.
pub generation: u64,
// input.rs
impl CanvasInput {
    /// `hand` makes a primary drag pan, like Space+drag.
    pub fn from_egui(ui: &egui::Ui, response: &egui::Response, hand: bool) -> CanvasInput;
}
```

- [ ] **Step 1：`edit_input.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use scene::editor::{Command, Cursor, Modifiers, PointerEvent, Tool};

    use super::*;

    fn frame(events: &[egui::Event]) -> FrameInput<'_> {
        FrameInput {
            events,
            canvas: egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0)),
            camera: Camera { scroll_x: 0.0, scroll_y: 0.0, zoom: 2.0 },
            modifiers: egui::Modifiers::NONE,
            panning: false,
            keyboard_taken: false,
            focused: true,
        }
    }

    fn button(x: f32, y: f32, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton { pos: egui::pos2(x, y), button: egui::PointerButton::Primary, pressed, modifiers }
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn pointer(x: f64, y: f64, modifiers: Modifiers) -> PointerEvent {
        PointerEvent { at: [x, y], modifiers, zoom: 2.0 }
    }

    #[test]
    fn pointer_events_become_scene_coordinates() {
        let events = [
            button(30.0, 40.0, true, egui::Modifiers::SHIFT),
            egui::Event::PointerMoved(egui::pos2(50.0, 60.0)),
            button(50.0, 60.0, false, egui::Modifiers::NONE),
        ];
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        assert_eq!(
            translate(&frame(&events), &mut PointerCapture::default()),
            vec![
                EditorInput::Down(pointer(10.0, 10.0, shift)),
                EditorInput::Move(pointer(20.0, 20.0, Modifiers::default())),
                EditorInput::Up(pointer(20.0, 20.0, Modifiers::default())),
            ]
        );
    }

    #[test]
    fn only_presses_that_start_on_the_canvas_reach_the_editor() {
        let mut capture = PointerCapture::default();
        let outside = [button(5.0, 5.0, true, egui::Modifiers::NONE), egui::Event::PointerMoved(egui::pos2(900.0, 900.0))];
        assert_eq!(translate(&frame(&outside), &mut capture), vec![]);
        let release = [button(900.0, 900.0, false, egui::Modifiers::NONE)];
        assert_eq!(translate(&frame(&release), &mut capture), vec![]);

        let press = [button(30.0, 40.0, true, egui::Modifiers::NONE)];
        let mut panning = frame(&press);
        panning.panning = true;
        assert_eq!(translate(&panning, &mut capture), vec![]);
        assert_eq!(translate(&frame(&release), &mut capture), vec![]);

        translate(&frame(&press), &mut capture);
        let beyond = [egui::Event::PointerMoved(egui::pos2(1000.0, 20.0)), button(1000.0, 20.0, false, egui::Modifiers::NONE)];
        assert_eq!(
            translate(&frame(&beyond), &mut capture),
            vec![
                EditorInput::Move(pointer(495.0, 0.0, Modifiers::default())),
                EditorInput::Up(pointer(495.0, 0.0, Modifiers::default())),
            ]
        );
    }

    #[test]
    fn losing_focus_mid_drag_releases_at_the_last_position() {
        let mut capture = PointerCapture::default();
        translate(&frame(&[button(30.0, 40.0, true, egui::Modifiers::NONE)]), &mut capture);
        let mut unfocused = frame(&[]);
        unfocused.focused = false;
        assert_eq!(translate(&unfocused, &mut capture), vec![EditorInput::Up(pointer(10.0, 10.0, Modifiers::default()))]);
        assert_eq!(translate(&unfocused, &mut capture), vec![]);
    }

    #[test]
    fn keys_map_to_tools_and_commands() {
        let ctrl = egui::Modifiers::COMMAND;
        let ctrl_shift = egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND };
        let none = egui::Modifiers::NONE;
        let events = [
            key(egui::Key::R, none),
            key(egui::Key::Num5, none),
            key(egui::Key::X, none),
            key(egui::Key::H, none),
            key(egui::Key::A, ctrl),
            key(egui::Key::Z, ctrl),
            key(egui::Key::Z, ctrl_shift),
            key(egui::Key::Y, ctrl),
            key(egui::Key::Delete, none),
            key(egui::Key::Backspace, none),
            key(egui::Key::Escape, none),
            key(egui::Key::Enter, none),
            key(egui::Key::R, ctrl),
        ];
        assert_eq!(
            translate(&frame(&events), &mut PointerCapture::default()),
            vec![
                EditorInput::Tool(Tool::Rectangle),
                EditorInput::Tool(Tool::Arrow),
                EditorInput::Tool(Tool::Freedraw),
                EditorInput::Tool(Tool::Hand),
                EditorInput::Command(Command::SelectAll),
                EditorInput::Command(Command::Undo),
                EditorInput::Command(Command::Redo),
                EditorInput::Command(Command::Redo),
                EditorInput::Command(Command::Delete),
                EditorInput::Command(Command::Delete),
                EditorInput::Command(Command::Escape),
                EditorInput::Command(Command::Finalize),
            ]
        );
        let mut typing = frame(&events);
        typing.keyboard_taken = true;
        assert_eq!(translate(&typing, &mut PointerCapture::default()), vec![]);
    }

    #[test]
    fn cursors_map_to_egui_icons() {
        assert_eq!(cursor_icon(Cursor::ResizeNwse), egui::CursorIcon::ResizeNwSe);
        assert_eq!(cursor_icon(Cursor::ResizeNs), egui::CursorIcon::ResizeVertical);
        assert_eq!(cursor_icon(Cursor::Pointer), egui::CursorIcon::PointingHand);
        assert_eq!(cursor_icon(Cursor::Crosshair), egui::CursorIcon::Crosshair);
    }
}
```

- [ ] **Step 2：`overlay.rs`、快取鍵與 generation 的測試**

`overlay.rs`：

```rust
#[cfg(test)]
mod tests {
    use scene::editor::Overlay;

    use super::*;

    fn colors() -> OverlayColors {
        OverlayColors { accent: egui::Color32::BLUE, selection: egui::Color32::LIGHT_BLUE, handle_fill: egui::Color32::WHITE }
    }

    #[test]
    fn scene_geometry_lands_in_screen_points() {
        let overlay = Overlay {
            handles: vec![[0.0, 0.0, 4.0, 4.0]],
            points: vec![[10.0, 5.0]],
            box_selection: Some([0.0, 0.0, 10.0, 10.0]),
            ..Overlay::default()
        };
        let camera = Camera { scroll_x: 1.0, scroll_y: 0.0, zoom: 2.0 };
        let shapes = shapes(&overlay, &camera, egui::pos2(10.0, 20.0), colors());
        let rects: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) => Some(r.rect),
                _ => None,
            })
            .collect();
        assert!(rects.contains(&egui::Rect::from_min_max(egui::pos2(12.0, 20.0), egui::pos2(20.0, 28.0))), "{rects:?}");
        assert!(rects.contains(&egui::Rect::from_min_max(egui::pos2(12.0, 20.0), egui::pos2(32.0, 40.0))), "{rects:?}");
        let centers: Vec<egui::Pos2> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Circle(c) => Some(c.center),
                _ => None,
            })
            .collect();
        assert_eq!(centers, vec![egui::pos2(32.0, 30.0)]);
    }
}
```

`render/cache.rs` 的測試模組：`key` 輔助函數加上 `version_nonce_bits: element.version_nonce().to_bits()`，並加：

```rust
    #[test]
    fn a_different_version_nonce_is_a_different_mesh() {
        let mut cache = SceneCache::new();
        let element = rect();
        let other = scene::Element::from_value(sample::with(
            sample::generic("rectangle", "a", [0.0, 0.0, 20.0, 10.0]),
            serde_json::json!({"versionNonce": 99}),
        ));
        let first = cache.mesh(&element, &key(&element, 0), "#ffffff").mesh.clone();
        let second = cache.mesh(&other, &key(&other, 0), "#ffffff").mesh.clone();
        assert!(!Arc::ptr_eq(&first, &second), "same id and version, different nonce");
    }
```

`tests/support/mod.rs` 加 `render_sequence`：同一個 `CanvasRenderer` 依序 `prepare` 每個 `(SceneFile, generation)`，只把最後一個畫出來讀回（`render_with_ppp` 改成呼叫它）。`tests/gpu_shapes.rs` 加：

```rust
#[test]
fn a_new_generation_forgets_cached_meshes() {
    // Same id, version and versionNonce with different geometry: only the generation says the
    // scene was replaced.
    let at = |x: f64| {
        sample::file(vec![sample::with(
            sample::generic("rectangle", "r", [x, 20.0, 30.0, 30.0]),
            json!({"roughness": 0, "backgroundColor": "#ffc9c9"}),
        )])
    };
    let image = support::render_sequence(vec![(at(10.0), 0), (at(60.0), 1)], Camera::default(), 100, 80, false);
    assert!(close(image.pixel(75, 35), [0xff, 0xc9, 0xc9], 2), "{:?}", image.pixel(75, 35));
    assert!(close(image.pixel(25, 35), [0xff, 0xff, 0xff], 0), "{:?}", image.pixel(25, 35));
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p app`
Expected: 編譯失敗。

- [ ] **Step 4：實作**

- `edit_input::translate`：依序處理事件。`PointerButton { button: Primary }` 按下時，`focused`、不在 `panning`、位置在 `canvas` 內才發 `Down` 並設 `pressed`；放開時只有 `pressed` 才發 `Up`。`PointerMoved` 在 `pressed` 或位置在 `canvas` 內時發 `Move`。座標：`camera.view_to_scene(pos - canvas.min)`。事件自帶的 modifiers 優先，沒有時用 `input.modifiers`；`ctrl` 是 `ctrl || command`。按鍵（`pressed && !repeat && !keyboard_taken`）：沒有 ctrl、alt 時 `V`／`1` Selection、`R`／`2` Rectangle、`D`／`3` Diamond、`O`／`4` Ellipse、`A`／`5` Arrow、`L`／`6` Line、`P`／`X`／`7` Freedraw、`H` Hand、`Delete`／`Backspace` Delete、`Escape`、`Enter` Finalize（Shift 可以按著）；ctrl 時 `A` SelectAll、`Z` Undo、Shift+`Z` 與 `Y` Redo。`focused` 為 `false` 而 `pressed` 時發一個 `Up`（位置是 `last`）並清掉 `pressed`。
- `overlay::shapes`：outline 用 `Shape::closed_line`（線寬 1、`accent`）；`selection_box` 用 `Shape::dashed_line`（虛線長 2 點、間隔 2 點）；控制點是 `Shape::rect_filled` 加 `rect_stroke`（填 `handle_fill`，邊 `accent`，圓角 2）；`points` 是半徑 5 點的 `Shape::circle_filled` 加 `circle_stroke`；`box_selection` 是填 `selection`（alpha 64）加 `accent` 邊的矩形。場景點轉螢幕：`origin + camera.scene_to_view(p)`。
- `render/cache.rs`：`MeshKey`、`ShapeKey` 加 `version_nonce_bits`；`render/plan.rs` 與 `render/gpu.rs` 裡建 key 的地方（網格、文字行、旋轉文字）一併帶入 `Element::version_nonce`。
- `render/gpu.rs`：`CanvasRenderer` 記住上一個 `generation`，不同時先清空 `SceneCache`、文字行快取、旋轉文字快取與 GPU 區段，再照常 `prepare`。
- `input.rs`：`from_egui` 的 `hand` 參數讓 `dragged_by(Primary)` 也平移。
- `NapkinApp`（原 `Viewer`）：載入成功時建立 `Editor::new((*document.file).clone(), SystemEnv)`，載入失敗時沒有 editor，行為同 M3。每幀：`CanvasInput::from_egui(ui, &response, editor.tool() == Tool::Hand)` 照舊套到相機；`panning` 是 Space 按著或 Hand 工具；`translate` 的結果依序送進 editor（`Tool` → `set_tool`，`Command` → `command`）；`ui.ctx().set_cursor_icon(cursor_icon(editor.cursor()))`（Space 按著或 Hand 工具時用 `Grab`）；`CanvasFrame { file: editor.file().clone(), generation: 0, .. }`；paint callback 之後 `ui.painter().extend(overlay::shapes(&editor.overlay(camera.zoom), ..))`；畫布上方中央用小字標示目前工具（`selection`、`rectangle` 等小寫名稱）；F12 面板加一行 `scene clones {n}`。`--bench` 的流程不變（Task 12 才加拖動）。
- Task 10 還不存檔：修改只在記憶體裡，關閉視窗就消失。

- [ ] **Step 5：驗證**

Run: `cargo test --workspace` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

手動（controller 在 Hyprland 上執行 `cargo run -p app --release -- crates/scene/tests/corpus/shapes.excalidraw`）：
1. `R` 拖出矩形、`O` 橢圓、`D` 菱形、`A` 拖出箭頭、`L` 連點三下再 `Enter`、`P` 畫筆畫，每個都立即畫出來，形狀建立後出現選取框與四個角的控制點（筆畫除外）。
2. 點選、Shift 加選、框選、拖動、拖角落縮放、拖箭頭端點、`Delete`、`Ctrl+Z`／`Ctrl+Shift+Z` 都照 excalidraw.com 的手感運作；游標在控制點上變成縮放游標。
3. Space+拖曳、中鍵拖曳、`H` 工具拖曳都只平移畫布，不選取也不移動元件。
4. F12 面板在拖動一個元件時 `scene clones` 只在每次開始拖動時加 1。

- [ ] **Step 6：Commit**

```bash
git add crates/app
git commit -m "Edit the canvas: route egui input to the scene editor and draw its overlay"
```

---

### Task 11：存檔整合

**Files:**
- Modify: `crates/app/src/main.rs`、`crates/app/src/napkin_app.rs`、`crates/app/src/lib.rs`
- Delete: `crates/app/src/document.rs`（由 `storage.rs` 取代）

**Interfaces:**
- Consumes: Task 9 `storage`、`autosave`、`writer`；Task 10 `NapkinApp`。
- Produces:

```rust
// napkin_app.rs
/// Spec §5.6: reload when the file on disk changed since napkin last read or wrote it, no
/// save is running and no unsaved element change would be lost.
pub fn should_reload(known_mtime: Option<SystemTime>, disk_mtime: Option<SystemTime>, saving: bool, unsaved: bool) -> bool;
```

- [ ] **Step 1：測試**

```rust
    #[test]
    fn reload_only_when_the_disk_changed_and_nothing_would_be_lost() {
        let t = |s| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        assert!(should_reload(Some(t(10)), Some(t(11)), false, false));
        assert!(should_reload(Some(t(10)), Some(t(9)), false, false), "an older file restored from a backup");
        assert!(should_reload(None, Some(t(5)), false, false), "the file appeared");
        assert!(!should_reload(Some(t(10)), Some(t(10)), false, false));
        assert!(!should_reload(Some(t(10)), None, false, false), "deleted: keep editing, the next save recreates it");
        assert!(!should_reload(Some(t(10)), Some(t(11)), true, false));
        assert!(!should_reload(Some(t(10)), Some(t(11)), false, true));
    }
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p app --lib napkin_app`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

- `main.rs`：`--bench` 時只用 `storage::load` 讀命令列的檔案（`Missing` 是空白場景，`Invalid` 顯示錯誤），不寫任何東西。否則 `Paths::from_env()`；有 `Paths` 時 `open_at_startup`，結果是 `Editable` 就 `remember`（失敗只寫 stderr）；沒有 `$HOME` 時命令列的檔案照常開啟與存檔、不記 `last`，沒有檔案時開空白場景、不存檔，並顯示通知 `"HOME is not set; changes are not saved"`。視窗標題是 `"{name} - napkin"`。
- `NapkinApp` 新增欄位：`path: Option<PathBuf>`（`None` 表示不存檔）、`unreadable: Option<String>`、`notice: Option<(String, Instant)>`、`autosave: Autosave`、`writer: Option<SaveWorker>`、`known_mtime: Option<SystemTime>`、`generation: u64`。可存檔時在 `new` 啟動 `SaveWorker::spawn`，`wake` 呼叫 `ctx.request_repaint()`。
- 每幀：先收 `writer.try_result()` → `autosave.finished`，成功時更新 `known_mtime`；再 `autosave.should_save(now, state, Trigger::Frame)`，成立時 `started` 並 `submit(SaveJob { path, file: editor.file().clone(), view })`；最後 `ctx.request_repaint_after(autosave.wake_after(now))`。`view` 由目前相機轉成 `NapkinView`。
- 焦點：`focused` 由 `true` 變 `false` 時，把這幀的 editor 輸入處理完（Task 10 的 `translate` 已補上 `Up`）後用 `Trigger::FocusLost` 檢查一次。由 `false` 變 `true` 時，重新讀主題（M3 已有），並在 `should_reload(known_mtime, storage::modified(path), autosave.in_flight(), autosave.has_unsaved_changes(revision))` 成立時 `storage::load`：`Parsed` → `editor.replace_file`、`generation += 1`、`autosave.reset`、`known_mtime` 更新；`Invalid` → `unreadable` 設為訊息，之後不再存檔；`Missing` → 不動。
- `unreadable` 時畫布照常顯示場景，但不把輸入送進 editor、不存檔，畫布中央上方顯示紅色錯誤訊息。啟動時就無法解析的檔案沒有 editor，照 M3 顯示錯誤。
- 右上角：文件名稱底下，`autosave.error()` 有值時顯示紅色 `"Save failed: {error}"`；`notice` 顯示 10 秒後消失（到期時 `request_repaint_after`）。
- `on_exit`：先停 pinch（M3 已有），再 `writer.shutdown()` 把結果交給 `autosave.finished`，然後 `autosave.should_save(now, state, Trigger::Exit)` 成立時在 UI 執行緒直接 `writer::save`，失敗寫 stderr。panic 時不存檔（spec §8），`on_exit` 不會被呼叫，不需要額外處理。

- [ ] **Step 4：驗證**

Run: `cargo test --workspace` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

手動（controller，在暫時的 `HOME` 目錄裡執行以免動到使用者的 `~/Documents`：`HOME=$(mktemp -d) cargo run -p app --release`）：
1. 第一次啟動建立 `$HOME/Documents/napkin/scratch.excalidraw` 與 `$HOME/.local/state/napkin/last`。
2. 畫一個矩形，停 1 秒後檔案出現該元件；拖動期間停住不放開，檔案不變。
3. 平移畫布後切到別的視窗，檔案的 `appState.napkin` 更新；只平移不切換視窗，檔案不變。
4. 關閉視窗（`hl.dsp.window.close()`）後重新啟動，開回同一個檔案與視角。
5. 在 napkin 失去焦點時用 `jq` 修改檔案裡矩形的 `x`，切回 napkin 後矩形移動、`Ctrl+Z` 沒有東西可以 undo。
6. `chmod a-w $HOME/Documents/napkin` 後修改元件，右上角出現紅色 `Save failed`；`chmod u+w` 後 5 秒內錯誤消失、檔案更新。
7. 把 `last` 指到一個無效的檔案後啟動：開出 scratch，並顯示提到那個檔案的通知。
8. 再把 `scratch.excalidraw` 也改成無效 JSON 後啟動：顯示錯誤、不能編輯，關閉後 `scratch.excalidraw` 的內容沒有變。

- [ ] **Step 5：Commit**

```bash
git add crates/app
git commit -m "Autosave to ~/Documents/napkin, reopen the last canvas and reload external changes"
```

---

### Task 12：拖動效能驗收

**Files:**
- Modify: `crates/app/src/bench.rs`、`crates/app/src/napkin_app.rs`

**Interfaces:**
- Consumes: Task 10 的 `NapkinApp` 與 editor；M3 的 `bench::camera_at`、`FrameStats`、`fixture`。
- Produces:

```rust
// bench.rs
/// The drag phase's length, after the camera script (`DURATION_S`).
pub const DRAG_S: f64 = 5.0;
/// The scene point the dragged element's grab point follows `t` seconds into the drag phase:
/// one revolution around `start` with a 200-unit radius. `None` once `t >= DRAG_S`.
pub fn drag_pointer_at(t: f64, start: [f64; 2]) -> Option<[f64; 2]>;
/// The element the drag phase moves: the last non-deleted rectangle, diamond or ellipse, and
/// the center of its box.
pub fn drag_target(file: &scene::SceneFile) -> Option<(usize, [f64; 2])>;
```

- [ ] **Step 1：測試**

```rust
    #[test]
    fn drag_phase_circles_the_start_and_picks_the_last_shape() {
        let start = [100.0, 50.0];
        assert_eq!(drag_pointer_at(0.0, start), Some(start));
        let quarter = drag_pointer_at(DRAG_S / 4.0, start).expect("dragging");
        assert!(((quarter[0] - start[0]).powi(2) + (quarter[1] - start[1]).powi(2)).sqrt() > 100.0);
        assert_eq!(drag_pointer_at(DRAG_S, start), None);

        let file = scene::sample::file(vec![
            scene::sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]),
            scene::sample::generic("ellipse", "b", [20.0, 0.0, 10.0, 20.0]),
            scene::sample::linear("arrow", "c", [0.0, 0.0], &[[0.0, 0.0], [5.0, 5.0]]),
        ]);
        assert_eq!(drag_target(&file), Some((1, [25.0, 10.0])));
        assert_eq!(drag_target(&scene::SceneFile::new()), None);
    }
```

`drag_pointer_at(0.0, start)` 必須剛好等於 `start`：用 `start + radius × (cos θ − 1, sin θ)` 的形式，和 `camera_at` 的平移階段相同。

- [ ] **Step 2：確認失敗**

Run: `cargo test -p app --lib bench`
Expected: 編譯失敗。

- [ ] **Step 3：實作**

- `--bench`：相機腳本結束時印出既有的 `bench:` 一行，重設 `FrameStats`，記下 `editor.scene_clones()`；有 `drag_target` 時把相機設成 `Camera::centered_on` 該元件的框（zoom 1）、工具設為 Selection、在中心 `pointer_down`，之後每幀 `pointer_move(drag_pointer_at(t))`，結束時 `pointer_up` 並印出 `"bench drag: frames {n}, interval p99 {x:.2} ms, cpu p99 {y:.2} ms, scene clones {c}"`（`c` 是拖動階段增加的次數）；沒有目標時印 `"bench drag: no shape to drag"`。兩行都印完後關閉視窗。
- 修掉 M3 留下的兩個 `--bench` 問題：`ViewportCommand::Close` 只送一次；檔案載入失敗或沒有 editor 時印 `"bench: no scene"` 並立即關閉，不再無限等待。
- `--bench` 不存檔（Task 11 已保證），拖動造成的修改只在記憶體裡。

- [ ] **Step 4：驗證**

Run: `cargo test --workspace` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

效能驗收（controller，release 建置）：

```bash
cargo run -p app --release --example perf_fixture -- target/perf.excalidraw 1
cargo run -p app --release -- target/perf.excalidraw --bench
```

Expected：兩行的 interval p99 都小於 12.5 ms；`bench drag` 的 `scene clones` 是 1；閒置 5 秒 CPU tick 為 0（M3 的量法）。沒通過就回到 Task 10 的實作找原因（例如每幀複製整個場景、拖動時重新三角化了其他元件），修到通過再 commit，commit message 附上量到的數字。

- [ ] **Step 5：Commit**

```bash
git add crates/app
git commit -m "Add a drag phase to --bench and fix its shutdown paths"
```

---

## M4a 完成條件

1. Task 1 到 12 的自動測試全部通過，包括 spec §9.2 的 undo property test（`undo_property.rs`）與互動狀態機測試（`editor_select.rs`、`editor_create.rs`）。
2. Task 12 的 `--bench`：平移縮放與拖動兩個階段的 interval p99 都小於 12.5 ms，拖動階段只複製場景一次，閒置時不重繪。
3. Task 10、11 的手動檢查全部通過。
4. 用 napkin 建立每種元件並存檔後，把檔案拖進 excalidraw.com：元件位置、大小、線條形狀一致，沒有錯誤訊息。

## 留給後續里程碑

- **M4b**
  - 工具列與屬性面板（編輯 `ItemStyle` 與已選元件的屬性），工具鎖定要不要做在這裡決定。
  - 文字工具、雙擊編輯文字、fcitx5（人工驗收 #2）、在含圖片的檔案改字後存檔（人工驗收 #4）、文字寬度量測。
  - 元件複製、貼上、複製一份（`excalidraw/clipboard`）；`Alt`+拖曳複製。
  - 圖層上下移：`History` 目前假設元件順序不變，要擴充成能記錄順序變化。
  - 橡皮擦。
  - M2 留下：新元件建構函數接受 `Some("")` 的箭頭頭部、`backgroundColor` 為 `""` 被當成有填色（屬性面板會寫這些值）；`Element::base_mut` 讓呼叫端能改 `kind`。
- **M5**
  - 被綁定元件移動與縮放時箭頭端點跟隨；載入時 `repairBindings` 那一組修復。
  - 容器文字重新換行、容器自動長高、有綁定文字時的最小尺寸；文字元件左右邊縮放（需要換行）。
  - 箭頭標籤照 `getBoundTextElementPosition` 定位，包括點選判定；拿掉 M4a 的標籤平移。
- **M6**：`Esc` 在沒有東西可取消時收起視窗；log 檔；畫布切換器；啟動時間量測。
- **M3 審查延後的項目**（M3 的執行帳本不在 git 裡，搬到這裡）
  - 沒有 `frameId` 為 `null` 或非字串的 typed 元件測試；`opacity`／`kind`／`version` 在 typed 分支用了多餘的 `unwrap_or`；`placement()` 把非數字的 `angle` 當 0，沒有測試。
  - 主題檔 `mode` 型別錯誤時訊息寫成 missing key；`Theme::load` 沒有直接測試。使用者目前的 omarchy 主題 `colors.toml` 是舊格式（沒有 `mode`、`selection`、`muted`），napkin 會用內建深色配色。
  - `normalized_zoom` 少了 Excalidraw `round()` 的 `Number.EPSILON`；`view_size` 從 `response.rect` 算了兩次；初始外框假設寬高非負；`input::apply` 的回傳值沒人用。
  - `tessellate.rs`：`None` 分支裡用不到的 `ElementShape::Placeholder`、用不到的 `"#000"` fallback、虛線與描邊分支重複幾行、沒有測試 Curve／Polygon／Path 填色走 EvenOdd；路徑座標是 NaN 時 lyon 的 `debug_assert!` 在 debug 建置 panic。旋轉公式在 `tessellate::transform`、`SceneRect::of_rotated` 與 M4a 的 `scene::geometry::rotate_point` 三處各有一份。
  - 沒有 id 的 `Raw` 元件在快取鍵共用空字串；連續的佔位框各自一批繪製。
  - `SceneCache::mesh()` 每個可見元件每幀呼叫約三次並複製 id；`StencilReset` 沒有 GPU 測試；測試輔助的 `Image::height` 用 `#[allow(dead_code)]`。
  - 文字行快取只在重設時清空；`rotated_quad_vertices` 裡用不到的 `local_center` fallback；多行文字置中、靠右沒有對照 canvas 的測試；`ROTATED_TEXT_FORMAT` 固定 `Rgba8Unorm`；raster scale 在上限邊界時可能超過貼圖上限一個像素。
  - 相機建立前的啟動幀算進 `FrameStats`；`interval_p99`／`cpu_p99` 每次呼叫都複製並排序 1200 筆；`--bench` 的縮放在第 5 秒跳到 1.25。
  - `--bench` 模式啟動了 `PinchListener` 卻不讀它的事件；只綁第一個 `wl_seat`、忽略 registry 的 `global_remove`。
  - 初期視窗大小變動時相機不重新置中（M6 的視窗規則在 map 時給定大小）；每幀規劃繪製清單的配置；mesh buffer 超出上限時每幀都寫一行 log。
- **M2 留下、M4a 不處理的項目**
  - `SceneFile` 寫回時 key 順序與原檔不同（spec §9.2 的語意比對忽略順序）。
  - 含孤立 UTF-16 surrogate 跳脫字元的檔案載入失敗，錯誤訊息只寫 invalid JSON。
  - `type: "line"` 卻帶 `elbowed: true` 的手改檔案會畫成 elbow 路徑。
  - 下次重新產生基準時補的案例：`rough_options` 的 line／freedraw 透明判斷差異、`.min(2.5)`、10／15 的尺寸門檻、未知的 `strokeStyle`、沒有 `roundness`；`shapes_generic` 的圓角 fallback 與異常尺寸、指數表示法的 path 數字；`freedraw_outline` 缺欄位或 `null` 的 `simulatePressure`、`variability`、`strokeOptions`、`streamline`，以及語料裡真實的滑鼠筆畫；產生器的 `source` 字串補 `points-on-curve`、`syncMovedIndices` 與 `syncInvalidIndices` 的摘要補 `versionNonce`、`updated`；git fetch 失敗留下半成品 `.cache/.git` 時訊息不清楚。
