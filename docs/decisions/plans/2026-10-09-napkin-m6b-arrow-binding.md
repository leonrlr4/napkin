# napkin M6b：箭頭綁定 Implementation Plan

> Historical record, frozen 2026-10-09. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把箭頭端點拖到形狀上就綁定；被綁的形狀搬移、縮放、旋轉、長高時，箭頭端點跟著停在外框外；箭頭標籤跟著箭頭重新定位。

**Architecture:** 規則都在 `scene`。`collision` 補上 `intersectElementWithLineSegment` 與它需要的曲線數學；`binding` 補上 `updateBoundPoint`／`updateBoundElements`（跟隨）與 simple 版的 `getBindingStrategyForDraggingBindingElementEndpoints`、`pointDraggingUpdates`、`bindOrUnbindBindingElement`（建立）；`bound_text` 補上箭頭標籤的位置與換行。`Editor` 在建立箭頭、拖端點、搬移／縮放／旋轉形狀、容器長高時呼叫它們，並在 `Overlay` 帶出綁定提示；`app` 只負責畫提示。

**Tech Stack:** 沿用現有：Rust 1.98.1（edition 2024）、eframe／egui 0.36.2、serde_json 1。不新增依賴。

**Spec:** 主 spec `docs/decisions/specs/2026-09-13-napkin-design.md` §5.8（箭頭綁定）與 §1.2 的「箭頭上的文字標籤」「舊版綁定格式」兩列；後者由本計畫的決定 6 修改。

**前置：** 分支 `m6b-arrow-binding`（從 `master` 的 `36b5468` 開出），在原本的 checkout 工作，不開 worktree。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號或計畫的決定編號（可以引用「spec §N」與 Excalidraw 函數名稱）。module doc 提到 Excalidraw 版本時寫「the pinned commit」。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 行為以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`。port 時看 JS 原始碼，不看計畫的摘要；兩者不一致時照 JS，並在回報寫出差異。
- 只 port `COMPLEX_BINDINGS` 關閉時走的路徑（`_simple` 結尾的函數），appState 取預設值：`isBindingEnabled: true`、`isMidpointSnappingEnabled: true`、`gridModeEnabled: false`、`bindMode: "orbit"`。
- `scene` 與 `rough` 不能依賴 egui、wgpu、glyphon。JS 數值語意走 `rough::js`；亂數與時間走 `scene::env::Env`。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫。
- `Element::Raw` 只能搬移、刪除、改 `index`：Raw 箭頭不跟隨也不綁定，Raw 元件不能當綁定目標。elbow arrow（`elbowed: true`）不跟隨、端點不綁定（主 spec §1.2）。
- 每個會改元件的使用者動作是一步 undo（主 spec §5.7）；拖動中途的綁定變化不另外記錄。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試沒有 adapter 時直接失敗，不跳過。改了 `tools/baseline/scene/cases.mjs` 或 `generate.mjs` 的任務要執行 `cd tools/baseline && npm run scene` 並 commit 產出的 JSON，其他群組的 JSON 必須完全不變。
- GUI 煙霧測試只做開視窗、截圖、關閉；用自己啟動的行程 PID 找視窗，`kill -TERM <pid>` 關閉；保留真實的 `WAYLAND_DISPLAY`／`XDG_RUNTIME_DIR`，只換 `HOME`；視窗只開幾秒。不用 `wtype`，不送任何按鍵或滑鼠事件，不讀寫剪貼簿。

## Review Focus

1. 箭頭兩端綁在同一個形狀上，搬移那個形狀：兩端都跟著動，箭頭不翻轉、不縮成一點（`moveMidPointsWithElement`）。Task 3 有測試。
2. 形狀和綁在它上面的箭頭一起被選取搬移：箭頭整支平移，端點不重新貼外框（`simultaneouslyUpdated`）。Task 3 有測試。
3. 寬或高小於 1 的形狀、長度為 0 的箭頭：`fixedPoint` 與端點都是有限值，不出現 NaN。Task 3、Task 4 有測試。
4. 開啟有舊版 `focus`／`gap` 綁定的檔案：不 panic，綁定轉成新格式後能跟隨；Raw 箭頭原封不動。Task 4 有測試。
5. 拖動一支綁好的箭頭後按一次 Ctrl+Z：箭頭、兩端綁定、目標的 `boundElements` 全部回到拖動前。Task 5 有測試。

## 寫計畫時做的決定

1. **綁定目標**：`rectangle`、`diamond`、`ellipse`、`image`、沒有 `containerId` 的 `text`，`locked` 的不算（`isBindableElement(el, false)`）。主 spec §5.8 沒列 `image`；M6d 已經把它做成型別化元件，JS 也讓它當目標，所以一起支援。
2. **不 port 的部分**：Alt 強制 inside 綁定、焦點把手（`arrows/focus.ts`）、elbow 的綁定與繞線、frame 裁切、格線吸附、`COMPLEX_BINDINGS` 的綁定模式計時器。這些分支在預設設定下走不到，或 napkin 沒有對應功能。
3. **按住 Ctrl 暫停綁定**：照 App.tsx，Ctrl 按著時 `isBindingEnabled` 為 false，拖動的端點解除綁定、不顯示提示。
4. **箭頭標籤的位置存回元件。** Excalidraw 渲染時才用 `getBoundTextElementPosition` 算箭頭標籤的位置，存檔裡的 `x`／`y` 可能是舊的；napkin 的渲染、選取、命中都讀存下來的 `x`／`y`，所以每次箭頭的點被綁定邏輯或拖端點改動後，把算出來的位置寫回標籤。excalidraw.com 讀檔時忽略箭頭標籤存的位置，互通不受影響。
5. **跟隨每次從手勢起點重算**，與搬移、縮放相同：`update_bound_elements` 讀目前（已移動的）形狀與箭頭的綁定，所以重複呼叫結果一樣。
6. **舊版綁定在載入時轉換**（修改主 spec §1.2 最後一列）。`restoreElements` 的 `repairBinding` 會把沒有 `mode` 的 `{focus, gap}` 綁定轉成 `{elementId, mode, fixedPoint}`，excalidraw.com 打開舊檔時也這樣做。napkin 在 `repair_on_load` 照做，代價是舊檔下次存檔時綁定欄位改成新格式；好處是舊檔的箭頭也會跟隨。有 `mode` 的綁定照 JS 只做 `normalizeFixedPoint`。
7. **提示的顏色**：外框與高亮的中點用 omarchy 主題的 accent（與選取框一致），沒高亮但顯示的中點用前景色、alpha 0.5；線寬照 JS `clamp(1.75, strokeWidth, 4) / max(0.25, zoom)` 場景單位。
8. **AI batch**：`update` 改了形狀的位置或大小時，呼叫 `update_bound_elements` 讓箭頭跟隨，取代現在「箭頭不動、只重算 `fixedPoint`」的 `refresh_bound_arrow_fixed_points`。`skills/napkin/SKILL.md` 一起改。
9. **基準**：四個新的 scene 基準群組 `intersect`、`arrow_label`、`bound_follow`、`arrow_binding`，都直接呼叫打包的 Excalidraw 原始碼。`arrow_binding` 用最小的 `app` 替身（`state`、`scene`、`getEffectiveGridSize: () => null`）驅動真正的 `LinearElementEditor.handlePointDragging`／`handlePointerMove` 與 `bindOrUnbindBindingElement`。

## 檔案結構

```
crates/scene/src/
  collision.rs     intersect_element_with_line_segment、曲線交點與弧長、binding_border_test、hovered_element_for_binding
  bound_text.rs    箭頭標籤的位置與換行
  binding.rs       跟隨、拖端點的綁定策略、建立與解除、提示、載入時轉換
  edit.rs          move_points（多點版 movePoints）、apply_drag 呼叫跟隨、repair_on_load 呼叫轉換
  transform.rs     縮放與旋轉呼叫跟隨
  editor/create.rs 建立箭頭時綁定
  editor/select.rs 拖端點時綁定
  editor/text.rs、editor/properties.rs  容器長高、線寬改變後呼叫跟隨
  editor/mod.rs    Overlay::binding_highlight
  batch/mod.rs     形狀更新後跟隨
crates/app/src/overlay.rs   畫綁定提示
tools/baseline/scene/       四個新群組
```

---

### Task 1：線段與元件外框的交點、找綁定目標

**Files:**
- Modify: `crates/scene/src/collision.rs`、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/intersect.json`

**Interfaces:**
- Consumes: `collision` 既有的 `deconstruct_rectanguloid`、`deconstruct_diamond`、`distance_to_element`、`linear_collision_shape`、`GeometryCache`。
- Produces:

```rust
// scene::collision
/// `intersectElementWithLineSegment(element, elementsMap, segment, offset)`: every point where
/// `segment` crosses `element`'s outline grown by `offset`, in the order JS returns them.
pub fn intersect_element_with_line_segment(
    geometry: &mut GeometryCache,
    element: &Element,
    segment: [[f64; 2]; 2],
    offset: f64,
) -> Vec<[f64; 2]>;

/// `hitElementItself` with `overrideShouldTestInside` exposed; `hit_element_itself` keeps
/// calling it with `false`.
pub fn hit_element_itself_with(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
    threshold: f64,
    test_inside: bool,
) -> bool;

/// `isBindableElement(element, false)`.
pub fn is_bindable(element: &Element) -> bool;

/// `getHoveredElementForBinding(point, elements, elementsMap, tolerance)`: position of the
/// bindable element to bind to, front to back, stopping at the first one with a
/// non-transparent background, then preferring the smallest diagonal.
pub fn hovered_element_for_binding(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
    tolerance: f64,
) -> Option<usize>;

// scene::geometry (or a new scene::curve module, implementer's choice; re-exported where Task 2 needs it)
pub fn curve_length(c: [[f64; 2]; 4]) -> f64;                                 // curveLength
pub fn curve_point_at_length(c: [[f64; 2]; 4], percent: f64, total: f64) -> [f64; 2]; // curvePointAtLength
pub fn curve_intersect_line_segment(c: [[f64; 2]; 4], l: [[f64; 2]; 2], iter_limit: usize) -> Vec<[f64; 2]>;
```

要讀的 JS：`packages/element/src/collision.ts` 的 `intersectElementWithLineSegment` 與它呼叫的四個 `intersect*WithLineSegment`、`bindingBorderTest`、`getAllHoveredElementAtPoint`、`getHoveredElementForBinding`、`hitElementItself`；`packages/math/src/curve.ts` 的 `curveIntersectLineSegment`（含 `solveWithAnalyticalJacobian`、預設 `tolerance 1e-2`、`iterLimit 4`）、`curveLength`、`curveLengthAtParameter`、`curvePointAtLength`（Legendre-Gauss 24 點的表在 `packages/math/src/constants.ts`）；`packages/math/src/ellipse.ts` 的 `ellipseSegmentInterceptPoints`；`packages/math/src/segment.ts` 的 `lineSegmentIntersectionPoints`。collision.rs 的 module doc 說這個 solver 只有綁定需要所以沒 port，現在 port 進來後一併改寫那段說明。

- [ ] **Step 1：失敗的單元測試**（`collision.rs` 的 `#[cfg(test)]`，照同檔既有的 helper）

```rust
#[test]
fn segment_meets_a_rectangle_grown_by_the_offset() {
    let rect = Element::from_value(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]));
    let hits = intersect_element_with_line_segment(
        &mut GeometryCache::default(), &rect, [[-20.0, 25.0], [50.0, 25.0]], 5.0,
    );
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!((hits[0][0] + 5.0).abs() < 1e-9 && (hits[0][1] - 25.0).abs() < 1e-9, "{hits:?}");
}

#[test]
fn segment_through_an_ellipse_hits_both_sides() {
    let ellipse = Element::from_value(sample::generic("ellipse", "e", [0.0, 0.0, 100.0, 50.0]));
    let mut hits = intersect_element_with_line_segment(
        &mut GeometryCache::default(), &ellipse, [[-10.0, 25.0], [110.0, 25.0]], 0.0,
    );
    hits.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert!((hits[0][0]).abs() < 1e-9 && (hits[1][0] - 100.0).abs() < 1e-9, "{hits:?}");
}

#[test]
fn a_filled_shape_stops_the_search_behind_it() {
    // A big transparent rectangle under a small filled one: the point is inside both, and
    // the filled one is in front, so it wins without looking further back.
    let elements = vec![
        Element::from_value(sample::generic("rectangle", "big", [0.0, 0.0, 300.0, 300.0])),
        Element::from_value(sample::with(
            sample::generic("rectangle", "small", [100.0, 100.0, 50.0, 50.0]),
            json!({"backgroundColor": "#ffc9c9"}),
        )),
    ];
    let hit = hovered_element_for_binding(&mut GeometryCache::default(), &elements, [120.0, 120.0], 15.0);
    assert_eq!(hit, Some(1));
}
```

Run: `cargo test -p scene --lib collision::tests`
Expected: 編譯失敗（函數不存在）。

- [ ] **Step 2：實作上面的函數，單元測試通過。** `hovered_element_for_binding` 跳過已刪除的元件（JS 的輸入本來就是 non-deleted）。

- [ ] **Step 3：`intersect` 基準群組**

bundle 入口加上 `intersectElementWithLineSegment`、`getHoveredElementForBinding`、`hitElementItself`（從 `@excalidraw/element/collision`）。案例形式 `[name, elements, call, args]`，`call` 是三者之一，輸出直接是回傳值（交點陣列、命中元件的 id 或 `null`、布林）。至少涵蓋：直角與圓角 rectangle、旋轉 0.6 rad 的 rectangle、直角與圓角 diamond、ellipse、text、image，各自 offset 0 與 6；線段完全在外框外（空陣列）；線段穿過圓角的角落（走曲線分支）；`getHoveredElementForBinding` 的前後兩個透明形狀（取小的）、前面有填色（停止）、在外框外 10 單位（tolerance 15 命中、tolerance 5 沒命中）、locked 形狀、有 `containerId` 的 text（都不算）；`hitElementItself` 的 `overrideShouldTestInside` 真假各一。`crates/scene/tests/baseline.rs` 新增 `intersect` 測試，交點陣列比對時順序也要相同。

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline intersect`
Expected: PASS，其他群組 JSON 不變。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene tools/baseline/scene
git commit -m "Port element/segment intersection and binding target lookup"
```

---

### Task 2：箭頭標籤的位置與換行

**Files:**
- Modify: `crates/scene/src/bound_text.rs`、`crates/scene/src/selection.rs`（只改過時的說明）、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/arrow_label.json`

**Interfaces:**
- Consumes: Task 1 的 `curve_length`、`curve_point_at_length`；`collision` 的 linear collision shape（`deconstructLinearOrFreeDrawElement` 的對應物）。
- Produces:

```rust
// scene::bound_text
/// `LinearElementEditor.getBoundTextElementPosition` (uncached): the label's top-left from its
/// `labelPosition` path parameter when it has one, else from `getBoundTextElementCenter`.
/// `None` for anything but a typed line or arrow.
pub fn arrow_label_position(
    geometry: &mut GeometryCache,
    arrow: &Element,
    label: &TextElement,
) -> Option<[f64; 2]>;

/// `handleBindTextResize`'s arrow branch (rewrap to `getBoundTextMaxWidth` of the arrow,
/// remeasure, grow the arrow's `height` when the text is taller than `getBoundTextMaxHeight`)
/// followed by writing `arrow_label_position` into the label's `x`/`y`. A no-op returning
/// false when `arrow` has no live label. Bumps what changed.
pub fn sync_arrow_label(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    arrow: usize,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool;
```

要讀的 JS：`linearElementEditor.ts` 的 `getBoundTextElementPosition`、`computeBoundTextElementPosition`、`getBoundTextElementCenter`、`getSegmentMidPoint`、`getPointAtPathParameter`、`getLinearElementPathMetrics`／`getLinearElementPathSegments` 與檔尾的 `pathSegment*` helper；`textElement.ts` 的 `handleBindTextResize`（只看箭頭會走的路徑）、`getBoundTextMaxWidth`／`getBoundTextMaxHeight` 的箭頭分支（`ARROW_LABEL_WIDTH_FRACTION = 0.7`、`ARROW_LABEL_FONT_SIZE_TO_MIN_WIDTH_RATIO = 11`）。`labelPosition` 是 text 元件上的可選欄位：確認 `TextElement` 能無損讀寫它（沒有就以 `Option<f64>` 加上，`serde(default, skip_serializing_if)`），corpus 測試不能退回 Raw。

- [ ] **Step 1：失敗的單元測試**

```rust
#[test]
fn a_two_point_arrow_label_centers_on_the_segment_midpoint() {
    let arrow = Element::from_value(sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [200.0, 0.0]]));
    let Element::Text(label) = Element::from_value(sample::text("t", [0.0, 0.0, 40.0, 20.0], "hi", Some("a"))) else { panic!() };
    let [x, y] = arrow_label_position(&mut GeometryCache::default(), &arrow, &label).unwrap();
    assert!((x - 80.0).abs() < 1e-9 && (y + 10.0).abs() < 1e-9, "{x} {y}");
}

#[test]
fn a_three_point_arrow_label_sits_on_the_middle_point() {
    let arrow = Element::from_value(sample::linear("arrow", "a", [10.0, 10.0], &[[0.0, 0.0], [100.0, 50.0], [200.0, 0.0]]));
    let Element::Text(label) = Element::from_value(sample::text("t", [0.0, 0.0, 40.0, 20.0], "hi", Some("a"))) else { panic!() };
    let [x, y] = arrow_label_position(&mut GeometryCache::default(), &arrow, &label).unwrap();
    assert!((x - 90.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9, "{x} {y}");
}
```

Run: `cargo test -p scene --lib bound_text::tests::a_`
Expected: 編譯失敗。

- [ ] **Step 2：實作，單元測試通過。** `sync_arrow_label` 對 elbow 箭頭也適用（JS 同樣處理），對 Raw 箭頭或 Raw 標籤是 no-op。

- [ ] **Step 3：`arrow_label` 基準群組**

bundle 入口加上 `LinearElementEditor`（`@excalidraw/element/linearElementEditor`）。每個案例建 `Scene`，呼叫 `handleBindTextResize(arrow, scene, null, false)`，再呼叫 `LinearElementEditor.getBoundTextElementPosition(arrow, label, elementsMap)`；輸出標籤的 `text`、`width`、`height`、算出的 `x`／`y`，與箭頭的 `height`。Rust 端呼叫 `sync_arrow_label` 後比對標籤的 `x`／`y`／`text`／`width`／`height` 與箭頭 `height`。案例：兩點、三點、四點直線箭頭；四點 `roundness: {type: 2}` 曲線箭頭；有 `labelPosition: 0.25` 的直線與曲線箭頭；標籤文字比 0.7 倍箭頭寬還長（會換行）；很短的箭頭（最小寬度 `fontSize * 11` 生效）；旋轉 0.5 rad 的箭頭。

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline arrow_label`
Expected: PASS，其他群組 JSON 不變。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene tools/baseline/scene
git commit -m "Place and rewrap arrow labels the way Excalidraw positions them"
```

---

### Task 3：被綁的形狀改變時箭頭跟隨

**Files:**
- Modify: `crates/scene/src/binding.rs`、`crates/scene/src/edit.rs`、`crates/scene/src/transform.rs`、`crates/scene/src/editor/select.rs`（呼叫端參數）、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/bound_follow.json`

**Interfaces:**
- Consumes: Task 1 的 `intersect_element_with_line_segment`、`hit_element_itself_with`；Task 2 的 `sync_arrow_label`；既有的 `binding::{linear_end_point, normalize_fixed_point, BASE_BINDING_GAP}`、`edit::move_linear_point`。
- Produces:

```rust
// scene::edit
/// `LinearElementEditor.movePoints` + `_updatePoints` (non-elbow) for several points at once:
/// `updates` are (index, new local point); point 0 moving shifts the rest and `x`/`y` the same
/// way `move_linear_point` does. `move_mid_points_with_element` is JS's option of that name.
/// Returns whether the element changed (and bumps it if so).
pub fn move_points(
    element: &mut Element,
    updates: &[(usize, [f64; 2])],
    move_mid_points_with_element: bool,
    env: &mut impl Env,
) -> bool;

// scene::binding
/// `getGlobalFixedPointForBindableElement`.
pub fn global_fixed_point(fixed_point: [f64; 2], target: &Element) -> Option<[f64; 2]>;

/// `updateBoundPoint`: the new local position of `arrow`'s `end` bound to `target`, or `None`
/// when JS returns null (no binding, a >2-point arrow whose other end is the one bound, or a
/// fresh arrow whose last point is still [0, 0]). Reads `elements` for the other end's target.
pub fn update_bound_point(
    geometry: &mut GeometryCache,
    elements: &[Element],
    arrow: &Element,
    end: LinearEnd,
    target: &Element,
    dragging: bool,
) -> Option<[f64; 2]>;

/// `updateBoundElements(changed, scene, {simultaneouslyUpdated})`: moves the bound ends of every
/// live, typed, non-elbow arrow in `changed`'s `boundElements` whose id is not in
/// `simultaneous`, then `sync_arrow_label`s it. Bindings without `mode` (legacy) are skipped.
pub fn update_bound_elements(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    changed: usize,
    simultaneous: &HashSet<String>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool;
```

`edit::apply_drag` 加上 `geometry: &mut GeometryCache` 與 `measure: &mut dyn TextMeasure` 參數，每個被搬移的非箭頭主目標呼叫一次 `update_bound_elements`，`simultaneous` 是全部主目標的 id（`dragSelectedElements` 的 `elementsToUpdate`）。`transform::resize_element`、`resize_elements`、`rotate_elements` 在 JS 呼叫 `updateBoundElements` 的同一個位置呼叫它（`rotate_elements` 需要加上 `geometry`、`measure` 參數）。呼叫端（`editor/select.rs`、baseline 測試、batch）跟著改簽名。

要讀的 JS：`binding.ts` 的 `updateBoundElements`、`updateBoundPoint`、`extractBinding`、`getGlobalFixedPointForBindableElement`、`boundElementsVisitor`、`bindableElementsVisitor`、`elementArea`、`BASE_ARROW_MIN_LENGTH`；`linearElementEditor.ts` 的 `movePoints`、`_updatePoints`、`createPointAt`；`dragElements.ts` 的 `dragSelectedElements`；`resizeElements.ts` 裡每個 `updateBoundElements` 呼叫點。

- [ ] **Step 1：失敗的單元測試**（`binding.rs`）

```rust
fn follow_scene(arrow_points: &[[f64; 2]], start: [f64; 2], both_ends_on_r: bool) -> SceneFile {
    // r: 100x100 at (0, 0), strokeWidth 2 (gap 6). Arrow `a` starts bound to r's center.
    let end_binding = if both_ends_on_r {
        json!({"elementId": "r", "mode": "orbit", "fixedPoint": [0.5001, 0.0]})
    } else {
        Value::Null
    };
    sample::file(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"strokeWidth": 2, "boundElements": [{"id": "a", "type": "arrow"}]}),
        ),
        sample::with(
            sample::linear("arrow", "a", start, arrow_points),
            json!({
                "startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [0.5001, 0.5001]},
                "endBinding": end_binding,
            }),
        ),
    ])
}

#[test]
fn moving_the_target_drags_the_bound_end_onto_its_outline() {
    let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
    file.elements[0].set_position(50.0, 0.0);
    let changed = update_bound_elements(
        &mut GeometryCache::default(), &mut file, 0, &HashSet::new(), &mut CharWidthMeasure, &mut FixedEnv,
    );
    assert!(changed);
    let start = linear_end_point(&file.elements[1], LinearEnd::Start).unwrap();
    // r now spans x 50..150; the start sits one gap (6) outside its right edge, on the line
    // toward the free end at (300, 50).
    assert!((start[0] - 156.0).abs() < 0.05 && (start[1] - 50.0).abs() < 0.05, "{start:?}");
    let end = linear_end_point(&file.elements[1], LinearEnd::End).unwrap();
    assert_eq!(end, [300.0, 50.0]);
}

#[test]
fn an_arrow_moved_together_with_its_target_is_left_alone() {
    let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
    let before = file.elements[1].clone();
    file.elements[0].set_position(50.0, 0.0);
    let simultaneous = HashSet::from(["r".to_owned(), "a".to_owned()]);
    update_bound_elements(&mut GeometryCache::default(), &mut file, 0, &simultaneous, &mut CharWidthMeasure, &mut FixedEnv);
    assert_eq!(file.elements[1], before);
}

#[test]
fn both_ends_on_one_target_follow_without_collapsing() {
    let mut file = follow_scene(&[[0.0, 0.0], [0.0, -56.0]], [50.0, 50.0], true);
    file.elements[0].set_position(200.0, 0.0);
    update_bound_elements(&mut GeometryCache::default(), &mut file, 0, &HashSet::new(), &mut CharWidthMeasure, &mut FixedEnv);
    let start = linear_end_point(&file.elements[1], LinearEnd::Start).unwrap();
    let end = linear_end_point(&file.elements[1], LinearEnd::End).unwrap();
    assert!(start.iter().chain(end.iter()).all(|v| v.is_finite()));
    assert!(start[0] > 190.0 && end[0] > 190.0, "{start:?} {end:?}");
}

#[test]
fn a_degenerate_target_keeps_points_finite() {
    let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
    if let Element::Rectangle(r) = &mut file.elements[0] { r.base.width = 0.0; r.base.height = 0.0; }
    update_bound_elements(&mut GeometryCache::default(), &mut file, 0, &HashSet::new(), &mut CharWidthMeasure, &mut FixedEnv);
    let start = linear_end_point(&file.elements[1], LinearEnd::Start).unwrap();
    assert!(start[0].is_finite() && start[1].is_finite(), "{start:?}");
}
```

（數值以 JS 為準：若 JS 算出的值與註解不同，改斷言並在回報說明。`set_position`、`sample`、`CharWidthMeasure`、`FixedEnv` 照現有寫法。）

Run: `cargo test -p scene --lib binding::tests`
Expected: 編譯失敗。

- [ ] **Step 2：實作 `move_points`、`global_fixed_point`、`update_bound_point`、`update_bound_elements`，單元測試通過。**

- [ ] **Step 3：接上搬移、縮放、旋轉。** 改 `apply_drag`、`resize_element`、`resize_elements`、`rotate_elements` 與所有呼叫端。既有測試必須照舊通過（既有的 `transform` 基準案例裡沒有新格式綁定的形狀被單獨改動，結果不該變）。

- [ ] **Step 4：`bound_follow` 基準群組**

bundle 入口加上 `dragSelectedElements`（`@excalidraw/element/dragElements`）。案例形式 `[name, elements, ids, op]`，`op` 是 `{drag: [dx, dy]}` 或 `{transform: [handle, pointer, center, flags]}`。drag 走 `dragSelectedElements(pointerDownState, selected, {x: dx, y: dy}, scene, {x: 0, y: 0}, null)`，`pointerDownState` 只需要 `originalElements`（id → 深拷貝）與 `drag.origin` 等它實際讀到的欄位（看 JS 決定）；transform 走既有 `transformElements` 的呼叫方式。輸出沿用 `transformOutput`，箭頭的 `startBinding`／`endBinding` 改成完整物件 `{elementId, mode, fixedPoint}`（只在這個群組；`transform` 群組的輸出不動），另外輸出箭頭標籤的 `x`／`y`／`text`。所有綁定都用新格式。案例至少涵蓋：

- 搬移：箭頭一端綁 rectangle、另一端自由；兩端綁不同形狀，只搬一個；兩端綁同一個形狀；形狀和箭頭一起搬；三點箭頭的起點綁定（只動端點，中間點不動）；`mode: "inside"` 綁定；有箭頭標籤的兩點箭頭；目標是 diamond、ellipse、旋轉 0.6 rad 的 rectangle、沒有箭頭頭的 line 型箭頭（`startArrowhead`、`endArrowhead` 都是 null 時走 `resolvedTarget` 的分支）。
- 縮放：被綁 rectangle 的 `se` 縮放；兩個形狀多選 `e` 縮放，其中一個綁著箭頭。
- 旋轉：單獨旋轉被綁的 rectangle。
- 極端：箭頭兩端的目標重疊（大的包小的，`elementArea` 門檻）；搬到使箭頭長度小於 `BASE_ARROW_MIN_LENGTH`。

`baseline.rs` 新增 `bound_follow` 測試：drag 呼叫 `apply_drag`、transform 呼叫對應的 `transform` 函數，比對同樣的欄位。

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline`
Expected: PASS，其他群組 JSON 不變。

- [ ] **Step 5：全部檢查並 commit**

```bash
git add crates/scene tools/baseline/scene
git commit -m "Make bound arrows follow moved, resized and rotated shapes"
```

---

### Task 4：拖端點時的綁定策略、建立與解除、載入時轉換

**Files:**
- Modify: `crates/scene/src/binding.rs`、`crates/scene/src/zindex.rs`、`crates/scene/src/edit.rs`（`repair_on_load`）、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/arrow_binding.json`

**Interfaces:**
- Consumes: Task 1、Task 3 的全部；既有 `binding::{bind_arrow, fixed_point_for}`、`edit::unbind_arrow_end`、`collision::is_point_in_element`。
- Produces:

```rust
// scene::binding
/// The parts of `LinearElementEditor.initialState` the binding strategy reads, carried by the
/// editor for one gesture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearDragState {
    /// `appState.newElement` is this arrow (creation, including click-to-add-point).
    pub new_arrow: bool,
    /// `initialState.origin`: the scene point the gesture started at.
    pub origin: Option<[f64; 2]>,
    /// `initialState.altFocusPoint`.
    pub alt_focus_point: Option<[f64; 2]>,
    /// `initialState.arrowOtherEndpointInitialBinding?.mode === "inside"`.
    pub other_end_started_inside: bool,
}

/// `appState.suggestedBinding`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuggestedBinding {
    pub target: usize,
    pub mid_point: Option<[f64; 2]>,
}

/// Per-event inputs shared by the drag and finalize calls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BindingInput {
    /// Scene pointer.
    pub pointer: [f64; 2],
    pub zoom: f64,
    /// Shift (`shouldRotateWithDiscreteAngle`) and a single point dragged.
    pub angle_locked: bool,
    /// `isBindingEnabled`: false while Ctrl is held.
    pub enabled: bool,
}

/// `pointDraggingUpdates` + `movePoints` + the `altFocusPoint`/`suggestedBinding` bookkeeping
/// of `handlePointDragging`/`handlePointerMove`: moves `indices` of the arrow at `arrow` by
/// `delta` (local units, already shift-locked by the caller), rebinding or unbinding the dragged
/// end on the arrow only (targets' `boundElements` are settled by `finalize_binding`), and
/// moving bound non-dragged ends with `update_bound_point`. A line just moves its points.
pub fn drag_points(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    arrow: usize,
    indices: &[usize],
    delta: [f64; 2],
    input: BindingInput,
    state: &mut LinearDragState,
    env: &mut impl Env,
) -> Option<SuggestedBinding>;

/// `bindOrUnbindBindingElement` with the dragged points `dragged` (index, local point): binds,
/// rebinds or unbinds each end the strategy names, keeping both sides of every binding in step,
/// then snaps ends whose strategy carried a focus point. `initial` is the creation-time call
/// on pointer-down (`initialBinding: true`).
pub fn finalize_binding(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    arrow: usize,
    dragged: &[(usize, [f64; 2])],
    input: BindingInput,
    state: &LinearDragState,
    initial: bool,
    env: &mut impl Env,
);

/// The arrow tool's hover suggestion (App.tsx's `isBindingElementType` hover branch).
pub fn hover_suggestion(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
    zoom: f64,
) -> Option<SuggestedBinding>;

/// `repairBinding` for every typed non-elbow arrow, run by `edit::repair_on_load`: a binding
/// with `mode` gets its `fixedPoint` normalized; a legacy one (no `mode`) whose target exists
/// becomes `{elementId, mode, fixedPoint}`; one that cannot be repaired becomes `null`.
pub fn repair_bindings(geometry: &mut GeometryCache, file: &mut SceneFile, env: &mut impl Env);

// scene::zindex
/// `moveArrowAboveBindable`: when the bindable element (with its label or container) under
/// `point` sits above `arrow`, moves the arrow just above it and resyncs indices.
pub fn move_arrow_above_bindable(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    arrow: usize,
    point: [f64; 2],
    env: &mut impl Env,
) -> usize; // the arrow's new position
```

`fixed_point_for` 加一個 `focus: Option<[f64; 2]>` 參數（`calculateFixedPointForNonElbowArrowBinding` 的 `focusPoint`），既有呼叫端傳 `None`；`bind_arrow` 加 `mode: &str` 與 `focus` 參數。

要讀的 JS：`binding.ts` 的 `maxBindingDistance_simple`、`getBindingStrategyForDraggingBindingElementEndpoints_simple`、`bindOrUnbindBindingElement`、`bindOrUnbindBindingElementEdge`、`bindBindingElement`、`unbindBindingElement`、`calculateFixedPointForNonElbowArrowBinding`、`snapToMid`（只在 elbow 用，不需要）；`utils.ts` 的 `projectFixedPointOntoDiagonal`、`getSnapOutlineMidPoint`、`getDiagonalsForBindableElement`；`linearElementEditor.ts` 的 `handlePointerMove`、`handlePointDragging`、`pointDraggingUpdates`；`zindex.ts` 的 `moveArrowAboveBindable`；`App.tsx` 建立箭頭的 pointer-down（約 10380–10470 行）、多點建立的 pointer-down（約 10230–10300 行）、hover（約 7898–7930 行）；`actionFinalize.tsx` 對箭頭的兩個 `bindOrUnbindBindingElement` 呼叫；`excalidraw/data/restore.ts` 的 `repairBinding`。

- [ ] **Step 1：失敗的單元測試**（`binding.rs`）

```rust
#[test]
fn dropping_a_new_arrow_end_near_a_shape_binds_it_on_both_sides() {
    // r: 100x100 at (200, 0). A new arrow from (0, 50) dragged to (195, 50), 5 units left of r.
    let mut file = sample::file(vec![
        sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
        sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [0.0, 0.0]]),
    ]);
    let mut geometry = GeometryCache::default();
    let mut state = LinearDragState { new_arrow: true, origin: Some([0.0, 50.0]), ..Default::default() };
    let input = BindingInput { pointer: [195.0, 50.0], zoom: 1.0, angle_locked: false, enabled: true };
    let suggested = drag_points(&mut geometry, &mut file, 1, &[1], [195.0, 0.0], input, &mut state, &mut FixedEnv);
    assert_eq!(suggested.map(|s| s.target), Some(0));
    finalize_binding(&mut geometry, &mut file, 1, &[(1, [195.0, 0.0])], input, &state, false, &mut FixedEnv);
    assert_eq!(file.elements[1].binding_target(LinearEnd::End), Some("r"));
    assert_eq!(file.elements[0].bound_elements(), vec![("a", "arrow")]);
}

#[test]
fn ctrl_held_drops_the_binding_of_the_dragged_end() {
    let mut file = sample::file(vec![
        sample::with(
            sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
        ),
        sample::with(
            sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
            json!({"endBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [-0.06, 0.5001]}}),
        ),
    ]);
    let mut geometry = GeometryCache::default();
    let mut state = LinearDragState::default();
    let input = BindingInput { pointer: [190.0, 50.0], zoom: 1.0, angle_locked: false, enabled: false };
    drag_points(&mut geometry, &mut file, 1, &[1], [-4.0, 0.0], input, &mut state, &mut FixedEnv);
    finalize_binding(&mut geometry, &mut file, 1, &[(1, [190.0, 0.0])], input, &state, false, &mut FixedEnv);
    assert_eq!(file.elements[1].binding_target(LinearEnd::End), None);
    assert!(file.elements[0].bound_elements().is_empty());
}

#[test]
fn a_legacy_binding_is_converted_on_load() {
    let mut file = sample::file(vec![
        sample::with(
            sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
        ),
        sample::with(
            sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
            json!({"endBinding": {"elementId": "r", "focus": 0, "gap": 6}}),
        ),
    ]);
    repair_bindings(&mut GeometryCache::default(), &mut file, &mut FixedEnv);
    let binding = file.elements[1].to_value()["endBinding"].clone();
    assert_eq!(binding["elementId"], "r");
    assert_eq!(binding["mode"], "orbit");
    assert!(binding["fixedPoint"][0].as_f64().unwrap().is_finite());
}
```

（同樣以 JS 為準。）

Run: `cargo test -p scene --lib binding::tests`
Expected: 新測試編譯失敗。

- [ ] **Step 2：實作上面的函數與 `move_arrow_above_bindable`，`repair_on_load` 在現有修復之後呼叫 `repair_bindings`；單元測試通過。** `repair_on_load` 沒有 `GeometryCache` 可用時就地建一個。

- [ ] **Step 3：`arrow_binding` 基準群組**

`generate.mjs` 寫一個手勢驅動器，照 App.tsx 的順序呼叫真正的 JS：

- `create`：`newArrowElement` 建在 `origin`（`id: "new"`、points `[[0,0],[0,0]]`），插入場景，照 App.tsx 呼叫 `bindOrUnbindBindingElement`（`newArrow: true, initialBinding: true`），建 `LinearElementEditor` 並設 `selectedPointsIndices: [1]`、`initialState.lastClickedPoint: 1`、`initialState.origin`；每個 `move` 呼叫 `LinearElementEditor.handlePointDragging(event, app, x, y, editor)`，把回傳的 `selectedLinearElement`／`suggestedBinding` 寫回 `app.state`；結束時照 `actionFinalize` 的第一個分支呼叫 `bindOrUnbindBindingElement`。
- `dragPoint`：既有箭頭，`selectedPointsIndices: [index]`、`pointerOffset` 由按下位置算出，其餘同上（`newArrow: false`）。
- `clickCreate`：多點建立，第一下之後每次移動呼叫 `handlePointerMove`，每次按下照 App.tsx 10230 行起的判斷決定加點或結束。

`event` 替身只有 `shiftKey`、`altKey: false`、`ctrlKey`、`metaKey: false`；`ctrlKey` 為真時 `app.state.isBindingEnabled` 設成 false。輸出：全部元件依場景順序的 `id`、`x`、`y`、`width`、`height`、`points`、完整的 `startBinding`／`endBinding`、`boundElements`，加上最後一次的 `suggestedBinding` 目標 id（或 null）。

`baseline.rs` 的 `arrow_binding` 測試用 Task 4 的 scene API 重現同一串事件（建立：`edit::append_element` 加上新箭頭，`finalize_binding(..., initial: true)`，每次移動算 `delta = 指標 - 偏移 - 目前的點` 再 `drag_points`，結束 `finalize_binding`），比對同樣的欄位。新箭頭的 seed、versionNonce 不比。

案例至少涵蓋：從空白拖到 rectangle 外框旁（orbit）；拖進 rectangle 裡面（inside）；從 rectangle 裡面按下拖到空白；從 A 拖到 B；終點拖回起點所在的形狀；Shift 鎖角；zoom 0.5（綁定距離變大）；終點停在 diamond、ellipse、text、旋轉的 rectangle 旁；拖動已綁定的終點離開（解除）與換到另一個形狀；三點箭頭拖中間點（兩端跟著 `update_bound_point`）；多點建立最後一下點在形狀上（立刻結束並綁定）；箭頭在形狀下層時建立（`moveArrowAboveBindable` 把它移上來）；Ctrl 按著。

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline arrow_binding`
Expected: PASS，其他群組 JSON 不變。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene tools/baseline/scene
git commit -m "Bind arrow endpoints while dragging and convert legacy bindings on load"
```

---

### Task 5：編輯器接上綁定

**Files:**
- Modify: `crates/scene/src/editor/create.rs`、`crates/scene/src/editor/select.rs`、`crates/scene/src/editor/text.rs`、`crates/scene/src/editor/properties.rs`、`crates/scene/src/editor/mod.rs`、`crates/scene/src/batch/mod.rs`、`skills/napkin/SKILL.md`
- Test: `crates/scene/tests/editor_binding.rs`（新檔）、`crates/scene/tests/editor_batch.rs`

**Interfaces:**
- Consumes: Task 3 的 `update_bound_elements`；Task 4 的全部。
- Produces:

```rust
// scene::editor
/// The binding hint drawn over the canvas while an arrow end can bind (`suggestedBinding`).
#[derive(Clone, Debug, PartialEq)]
pub struct BindingHighlight {
    /// The target's outline in scene coordinates, closed, sampled densely enough to draw as a
    /// polyline (rounded corners and ellipses included), rotated with the target.
    pub outline: Vec<[f64; 2]>,
    /// `clamp(1.75, strokeWidth, 4) / max(0.25, zoom)`, scene units.
    pub stroke_width: f64,
    /// The four side midpoints (`getSnapOutlineMidPoint`'s candidates) that JS draws, each with
    /// whether it is the highlighted one; empty when the pointer is inside the target or the
    /// angle is locked.
    pub midpoints: Vec<([f64; 2], bool)>,
    /// `4 / zoom`.
    pub midpoint_radius: f64,
}

pub struct Overlay {
    // ...existing fields...
    pub binding_highlight: Option<BindingHighlight>,
}
```

接線（每一處都照對應的 JS 呼叫點）：

- **建立（拖動）**：`start_linear` 在加入箭頭後呼叫 `finalize_binding(..., initial: true)`，`LinearState` 帶 `LinearDragState { new_arrow: true, origin: Some(event.at), .. }`；`apply_linear_drag_move` 改成：先照現有 `move_linear_point` 的方式（含 Shift 鎖角）算出新的點，換算成 `delta` 交給 `drag_points`，再 `move_arrow_above_bindable`（更新 `state.position`）；`finish_linear_drag` 結束前呼叫 `finalize_binding`。線段（`Tool::Line`）不綁定、不顯示提示。
- **建立（點擊加點）**：`multi_point_move` 對箭頭走 `drag_points`（最後一點）；`multi_point_pointer_down` 照 App.tsx 10236–10290 行，`end.mode === "orbit"` 且起點不是綁同一個目標、或兩端綁同一目標且終點在外框外時立刻結束；`finish_multi_point` 結束前 `finalize_binding`。
- **拖端點**：`PointDragState` 帶 `LinearDragState`（`origin` 為按下位置，`other_end_started_inside` 照 JS 由另一端按下時的綁定決定），`apply_point_drag_move` 走 `drag_points`，`pointer_up` 時 `finalize_binding`。拖完把箭頭標籤 `sync_arrow_label`。
- **hover**：工具是 Arrow 且沒有手勢時，`pointer_move` 存 `hover_suggestion` 的結果；工具切換或手勢開始時清掉。
- **Ctrl**：每個事件的 `BindingInput.enabled = !event.modifiers.ctrl`。
- **長高與屬性**：`commit_text`（以及 M6a 讓容器長高的其他路徑）與屬性面板改了 `strokeWidth`、字級、字型等會改變形狀外框或間距的屬性後，對被改的元件呼叫 `update_bound_elements`（JS：`textWysiwyg.tsx` 的兩處、`actionProperties.tsx`、`actionStyles.tsx`）。
- **Overlay**：`overlay()` 由目前的 `SuggestedBinding` 產生 `binding_highlight`；外框取樣照 `collision` 的 deconstruct 結果（直線取兩端點，曲線每段取 8 點以上），ellipse 取 64 點以上。
- **batch**：`update` 改了形狀的 `x`／`y`／`width`／`height` 後呼叫 `update_bound_elements`（`simultaneous` 為同一批 `update` 也改到的元件 id），刪掉 `refresh_bound_arrow_fixed_points` 及只為它存在的程式碼；`SKILL.md` 改寫對應的說明。

- [ ] **Step 1：失敗的編輯器測試**（`crates/scene/tests/editor_binding.rs`，照 `editor_create.rs` 的事件 helper）

```rust
#[test]
fn drawing_an_arrow_onto_a_rectangle_binds_it_and_one_undo_removes_everything() {
    let mut editor = editor_with(vec![sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0])]);
    editor.set_tool(Tool::Arrow);
    drag(&mut editor, [0.0, 50.0], [195.0, 50.0]);
    let arrow = last_element(&editor);
    assert_eq!(arrow.binding_target(LinearEnd::End), Some("r"));
    assert_eq!(element(&editor, "r").bound_elements(), vec![(arrow.id().unwrap(), "arrow")]);
    editor.command(Command::Undo);
    assert_eq!(editor.file().elements.len(), 1);
    assert!(element(&editor, "r").bound_elements().is_empty());
}

#[test]
fn dragging_a_bound_rectangle_moves_the_arrow_end_and_undo_restores_it() {
    let mut editor = editor_with(bound_pair()); // r at (200,0) 100x100, arrow a bound at its end
    let before = element(&editor, "a").clone();
    editor.set_tool(Tool::Selection);
    drag(&mut editor, [250.0, 50.0], [250.0, 150.0]);
    let end = scene::binding::linear_end_point(element(&editor, "a"), LinearEnd::End).unwrap();
    assert!(end[1] > 100.0, "{end:?}");
    editor.command(Command::Undo);
    assert_eq!(element(&editor, "a"), &before);
}

#[test]
fn hovering_with_the_arrow_tool_shows_the_binding_highlight() {
    let mut editor = editor_with(vec![sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0])]);
    editor.set_tool(Tool::Arrow);
    hover(&mut editor, [195.0, 50.0]);
    let highlight = editor.overlay(1.0).binding_highlight.expect("highlight");
    assert!(highlight.outline.len() >= 4);
    hover(&mut editor, [0.0, 0.0]);
    assert!(editor.overlay(1.0).binding_highlight.is_none());
}

#[test]
fn ctrl_held_draws_an_unbound_arrow() {
    let mut editor = editor_with(vec![sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0])]);
    editor.set_tool(Tool::Arrow);
    drag_with(&mut editor, [0.0, 50.0], [195.0, 50.0], Modifiers { ctrl: true, ..Default::default() });
    assert_eq!(last_element(&editor).binding_target(LinearEnd::End), None);
}

#[test]
fn a_label_that_grows_its_container_drags_the_bound_arrow_along() {
    // r: 100x50 at (0, 0); arrow a starts below r (0, 120) and ends bound to r's bottom
    // (`fixedPoint` [0.5001, 1.12]). Double-click r, type four short lines, commit: r grows
    // past y = 50, and a's end must stay below r's new bottom edge.
}
```

最後一個測試用 `editor_text.rs` 現有的雙擊、輸入、commit helper 寫完，斷言：commit 後 r 的高度大於 50，`a` 的終點 y 大於 r 新的下緣，且整個編輯是一步 undo。`editor_batch.rs` 加一個測試：batch `update` 搬移綁著箭頭的形狀後，箭頭端點跟著動且 `fixedPoint` 不變。

Run: `cargo test -p scene --test editor_binding`
Expected: 失敗（沒有綁定）。

- [ ] **Step 2：照上面的接線清單實作，測試通過，既有測試照舊通過**（`editor_create.rs` 裡畫箭頭的測試如果因為 `moveArrowAboveBindable` 或綁定而改變結果，以 JS 行為為準更新並在回報列出）。

- [ ] **Step 3：全部檢查並 commit**

```bash
git add crates/scene skills/napkin/SKILL.md
git commit -m "Bind arrows in the editor and show the binding hint"
```

---

### Task 6：畫綁定提示

**Files:**
- Modify: `crates/app/src/overlay.rs`（必要時 `crates/app/src/theme.rs`）
- Test: `crates/app/src/overlay.rs` 的 `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 5 的 `Overlay::binding_highlight`。
- Produces: 無新的公開介面。

- [ ] **Step 1：失敗的單元測試**：給一個有 `binding_highlight`（四點外框、兩個中點其中一個高亮）的 `Overlay`，`shapes(...)` 回傳的形狀裡有一條 accent 色的封閉折線，線寬等於 `stroke_width * zoom` 螢幕點；高亮中點是 accent 色實心圓、半徑 `midpoint_radius * zoom`；另一個中點是前景色 alpha 128 的實心圓。

Run: `cargo test -p app --lib overlay`
Expected: FAIL。

- [ ] **Step 2：實作，測試通過。** 提示畫在選取框之下、框選矩形之上（與 JS 的 `renderInteractiveScene` 順序一致：綁定提示先於選取）。

- [ ] **Step 3：煙霧測試。** `cargo build --release`，用暫時的 `HOME` 與 Task 5 測試用的場景檔（rectangle 加一支綁在它上面的箭頭）開 napkin，截圖確認畫面正常、箭頭端點停在外框外，`kill -TERM` 關閉。截圖路徑寫在回報裡。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Draw the arrow binding hint"
```

---

## 自我檢查

- 主 spec §5.8「建立」：Task 4（策略、建立、解除）、Task 5（建立箭頭、拖端點、點擊加點、hover 提示）、Task 6（畫提示）。綁定目標依決定 1 多了 image。
- 「跟隨」：Task 3（搬移、縮放、旋轉、`orbit` 與 `inside`、間距）、Task 5（長高、屬性、batch）。
- 「刪除」：M4a 已實作（`delete_selection` 的 `fixBindingsAfterDeletion`），本計畫不動；Task 5 的 undo 測試涵蓋兩端一致。
- 「兩端一致」：Task 4 的 `finalize_binding` 同時寫箭頭與目標；拖動中途只寫箭頭，與 JS 相同，手勢結束時一致。
- §1.2「箭頭上的文字標籤」：Task 2（位置、換行）、Task 3 與 Task 5（跟隨與拖端點後重新定位）。
- §1.2「舊版綁定格式」：依決定 6 改成載入時轉換，Task 4。elbow arrow 仍不跟隨（Task 3 跳過）。
- M6a 延後項目「編輯器裡容器長高不更新箭頭」：Task 5。
