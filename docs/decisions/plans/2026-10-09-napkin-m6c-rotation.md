# napkin M6c：旋轉 Implementation Plan

> Historical record, frozen 2026-10-09. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 選取框上方有旋轉控制點，拖動可以旋轉單一元件或多選（Shift 鎖 15°）；旋轉過的元件照常顯示控制點並沿自己的軸縮放。

**Architecture:** 規則都在 `scene::transform`：新增 `rotate_elements`（`rotateSingleElement`／`rotateMultipleElements`），控制點計算改成 `getTransformHandlesFromCoords` 的旋轉版（每個控制點仍是軸對齊的方塊，只有中心點隨角度旋轉），`handle_at` 先測旋轉控制點、再測角落、再測旋轉後的四邊。縮放公式 M4a 已經帶著角度項 port 過，這裡拿掉「有旋轉就不給控制點」的閘門，並用 JS 基準驗證旋轉過的元件縮放結果。`Editor` 的選取工具多一個旋轉手勢；`app` 畫圓形旋轉控制點並對應游標。

**Tech Stack:** 沿用現有：Rust 1.98.1（edition 2024）、eframe／egui 0.36.2、serde_json 1。不新增依賴。

**Spec:** `docs/decisions/specs/2026-10-09-napkin-rotation-images-design.md` §3（M6c）。主 spec `docs/decisions/specs/2026-09-13-napkin-design.md` 的 §1.2 第一列由它修改。

**前置：** 分支 `m6c-rotation`（從 `master` 的 `5edcb9e` 開出，第一個 commit 是 spec），在原本的 checkout 工作，不開 worktree。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號或計畫的決定編號（可以引用「spec §N」與 Excalidraw 函數名稱）。module doc 提到 Excalidraw 版本時寫「the pinned commit」，不要再複製 commit hash。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 行為以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`。port 時看 JS 原始碼，不看計畫的摘要；兩者不一致時照 JS，並在回報寫出差異。
- `scene` 與 `rough` 不能依賴 egui、wgpu、glyphon。
- JS 數值語意走 `rough::js`（`Math.atan2` 用 `rough::js::atan2`，`Math.round` 用 `math_round`）；亂數與時間走 `scene::env::Env`。JS 的 `%` 對負數保留被除數的正負號，與 Rust 的 `%` 相同。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫。
- `Element::Raw` 只能搬移、刪除、改 `index`：選取裡有 Raw 時不顯示旋轉控制點，也不旋轉它。elbow arrow 不能旋轉、也沒有控制點（維持現狀）。
- 箭頭綁定跟隨屬於 M6b：旋轉被綁定的形狀時箭頭不跟著動（與搬移相同）；旋轉一支綁定中的箭頭時照 JS 解除它的綁定。
- 每個會改元件的使用者動作是一步 undo（主 spec §5.7）。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試沒有 adapter 時直接失敗，不跳過。改了 `tools/baseline/scene/cases.mjs` 或 `generate.mjs` 的任務要執行 `cd tools/baseline && npm run scene` 並 commit 產出的 JSON，其他群組的 JSON 必須完全不變。
- GUI 煙霧測試只做開視窗、截圖、關閉；用自己啟動的行程 PID 找視窗，`kill -TERM <pid>` 關閉；保留真實的 `WAYLAND_DISPLAY`／`XDG_RUNTIME_DIR`，只換 `HOME`；視窗只開幾秒。不用 `wtype`，不送任何按鍵或滑鼠事件。

## 寫計畫時做的決定

1. **控制點仍用 `Bounds` 表示。** JS 的 `generateTransformHandle` 只旋轉控制點的中心，方塊本身維持軸對齊，命中判定 `isInsideTransformHandle` 也是軸對齊的矩形測試。所以 `corner_handles` 與 `Overlay::handles` 的型別不變，只是位置跟著角度走。
2. **旋轉控制點獨立一個欄位。** `HandleKind` 新增 `Rotation`；`Overlay` 新增 `rotation_handle: Option<Bounds>`，`app` 畫成圓形（Excalidraw `renderTransformHandles` 對 rotation 畫圓）。`handles` 只放縮放用的方塊。
3. **旋轉每次都從手勢開始時的場景重算**，與縮放、拖動相同。`rotateMultipleElements` 在 JS 裡是逐次增量（用目前元件的角度與原始角度相減），從起點重算的結果相同，因為它旋轉的量是絕對的 `centerAngle`。
4. **多選的旋轉中心**是按下時選取範圍的共同外框中心（`pointerDownState.resize.center`），存在手勢狀態裡，移動中不重算。
5. **游標。** 旋轉控制點顯示 `Cursor::Grab`（JS 的 `"grab"`，egui 的 `CursorIcon::Grab`）。旋轉過的元件的縮放游標照 `rotateResizeCursor` 依角度換成對應的四種之一。
6. **基準。** 新增 scene 基準群組 `transform`，直接呼叫打包的 `transformElements`（單選、多選、旋轉、縮放都走它），輸出每個元件的 `x`、`y`、`width`、`height`、`angle`，線段另加 `points`、文字另加 `fontSize`。這同時驗證 M4a 已有的縮放公式在角度不為 0 時是否正確；不正確就在本里程碑修。

## 檔案結構

```
crates/scene/src/
  transform.rs           rotate_elements、旋轉控制點、旋轉後的控制點位置與四邊命中、游標方向
  editor/select.rs       旋轉手勢
  editor/mod.rs          Overlay::rotation_handle、Cursor::Grab
crates/scene/tests/
  baseline.rs（transform 群組）、editor_select.rs
crates/app/src/
  overlay.rs             畫旋轉控制點
  edit_input.rs          Cursor::Grab 對應
tools/baseline/scene/    transform 群組
```

---

### Task 1：`rotate_elements` 與 `transform` 基準群組

**Files:**
- Modify: `crates/scene/src/transform.rs`、`tools/baseline/scene/cases.mjs`、`tools/baseline/scene/generate.mjs`、`crates/scene/tests/baseline.rs`
- Create（產生）: `crates/scene/tests/baseline/transform.json`

**Interfaces:**
- Consumes: 既有的 `transform::{resize_element, resize_elements, ResizeOptions, HandleKind}`、`bound_text::bound_text_position`、`edit` 裡的 `unbind_arrow_end`（需要時改成 `pub(crate)`）。
- Produces:

```rust
// scene::transform
/// `transformElements`' rotation branch: `rotateSingleElement` when `targets` (ascending
/// positions, bound text excluded the same way `resize_elements` excludes it) holds one
/// element, `rotateMultipleElements` around `center` otherwise. Recomputed from `start` on
/// every call, so repeated calls with the same arguments are idempotent. `discrete` is Shift
/// (`shouldRotateWithDiscreteAngle`). Returns whether anything changed.
#[expect(clippy::too_many_arguments, reason = "mirrors transformElements' rotation contract")]
pub fn rotate_elements(
    file: &mut SceneFile,
    start: &SceneFile,
    targets: &[usize],
    pointer: [f64; 2],
    center: [f64; 2],
    discrete: bool,
    env: &mut impl Env,
) -> bool;
```

要讀的 JS：`packages/element/src/resizeElements.ts` 的 `transformElements`、`rotateSingleElement`、`rotateMultipleElements`（含它對綁定文字、綁定箭頭的處理），`packages/math/src/angle.ts` 的 `normalizeRadians`，`packages/common/src/constants.ts` 的 `SHIFT_LOCKING_ANGLE`。`resizeMultipleElements` 的 `keepAspectRatio` 在任一元件 `angle !== 0` 時成立，napkin 現在少了這個條件，一併補上。

- [ ] **Step 1：失敗的單元測試**（`transform.rs` 的 `#[cfg(test)]`，`sample` 與既有測試的 helper 照同檔寫法）

```rust
#[test]
fn rotating_a_single_element_sets_its_angle_from_the_pointer() {
    let start = sample::file(vec![sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0])]);
    let mut file = start.clone();
    // Pointer straight right of the center (50, 25): 5π/2 + atan2(0, 50), normalized.
    assert!(rotate_elements(&mut file, &start, &[0], [100.0, 25.0], [50.0, 25.0], false, &mut FixedEnv));
    let p = file.elements[0].placement().unwrap();
    assert!((p.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12, "{}", p.angle);
    assert_eq!([p.x, p.y, p.width, p.height], [0.0, 0.0, 100.0, 50.0]);
}

#[test]
fn shift_snaps_to_fifteen_degrees() {
    let start = sample::file(vec![sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0])]);
    let mut file = start.clone();
    rotate_elements(&mut file, &start, &[0], [100.0, 30.0], [50.0, 25.0], true, &mut FixedEnv);
    let angle = file.elements[0].placement().unwrap().angle;
    assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{angle}");
}

#[test]
fn rotating_two_elements_turns_them_around_the_common_center() {
    let start = sample::file(vec![
        sample::generic("rectangle", "a", [0.0, 0.0, 40.0, 20.0]),
        sample::generic("rectangle", "b", [60.0, 0.0, 40.0, 20.0]),
    ]);
    let mut file = start.clone();
    rotate_elements(&mut file, &start, &[0, 1], [100.0, 10.0], [50.0, 10.0], false, &mut FixedEnv);
    let a = file.elements[0].placement().unwrap();
    let b = file.elements[1].placement().unwrap();
    // a's center (20, 10) turns a quarter clockwise around (50, 10) to (50, -20).
    assert!((a.x - 30.0).abs() < 1e-9 && (a.y + 30.0).abs() < 1e-9, "{a:?}");
    assert!((b.x - 30.0).abs() < 1e-9 && (b.y - 30.0).abs() < 1e-9, "{b:?}");
    assert!((a.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn a_container_label_turns_with_its_container() {
    let start = sample::file(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::with(
            sample::text("t", [26.0, 12.5, 48.0, 25.0], "hi", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
    ]);
    let mut file = start.clone();
    rotate_elements(&mut file, &start, &[0], [100.0, 25.0], [50.0, 25.0], false, &mut FixedEnv);
    let t = file.elements[1].placement().unwrap();
    assert!((t.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}
```

（`FixedEnv`、`sample::text` 的參數以同檔與 `sample.rs` 現有寫法為準。）

Run: `cargo test -p scene --lib transform::tests::rotat`
Expected: 編譯失敗（`rotate_elements` 不存在）。

- [ ] **Step 2：實作 `rotate_elements` 與 `resize_elements` 的旋轉 `keepAspectRatio` 條件，單元測試通過。**

- [ ] **Step 3：`transform` 基準群組**

`generate.mjs` 的 bundle 入口加上 `export { transformElements } from "@excalidraw/element/resizeElements";`（`transformElements` 若不在該模組匯出，從它實際所在的模組匯出）。每個案例：建 `Scene`，`originalElements` 是元素的深拷貝 Map，`selectedElements` 是 `ids` 對應的場景元件，呼叫 `transformElements(originalElements, handle, selected, scene, shift, fromCenter, keepAspect, pointer[0], pointer[1], center[0], center[1])`，輸出全部元件的 `x`、`y`、`width`、`height`、`angle`（線段另加 `points`、文字另加 `fontSize`、`text`）。`cases.mjs` 新增：

```js
/**
 * [name, elements, ids, handle, pointer, center, {shift, fromCenter, keepAspect}] for the
 * transform group. `center` is the common-bounds center a multi-element rotation turns
 * around (ignored otherwise). `handle` is "rotation" or a resize direction.
 */
export const transformCases = [ /* 見下方清單 */ ];
```

案例至少涵蓋：

- 旋轉：rectangle、ellipse、diamond、text、freedraw、三點 line 各一；Shift 鎖角兩個（一個剛好落在倍數附近、一個在兩倍數中間）；指標在中心正上方（角度 0）、正左方、左下方；帶標籤的容器；兩個與三個元件的多選（其中一個本身已旋轉 0.4 rad）；多選中有帶標籤的容器。
- 旋轉過的元件縮放：rectangle `angle: 0.6` 對 `n`、`s`、`e`、`w`、`nw`、`ne`、`sw`、`se` 各一；`angle: 2.5` 的 `se`；`keepAspect: true` 一個、`fromCenter: true` 一個；拉過對邊翻轉一個；旋轉過的 text 角落縮放一個；旋轉過的帶標籤容器 `e` 一個；旋轉過的三點 line `se` 一個。
- 多選縮放：其中一個元件 `angle: 0.4` 的兩元件 `se`（驗證強制等比例）。

字寬照既有的 `CharWidthMeasure` 公式。`crates/scene/tests/baseline.rs` 新增 `transform` 測試：`handle == "rotation"` 呼叫 `rotate_elements`，否則單選呼叫 `resize_element`、多選呼叫 `resize_elements`（參數對應照既有呼叫端），輸出同樣的欄位。

Run: `cd tools/baseline && npm run scene && cd ../.. && cargo test -p scene --test baseline transform`
Expected: PASS。若旋轉過的縮放案例不通過，照 JS 修 `transform.rs` 裡與角度有關的分支（M4a 只用 `angle == 0` 驗證過它們），直到全部通過；其他群組的 JSON 不變。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/scene tools/baseline/scene
git commit -m "Port element rotation and verify rotated resizing against Excalidraw"
```

---

### Task 2：旋轉後的控制點、旋轉控制點、命中判定與游標

**Files:**
- Modify: `crates/scene/src/transform.rs`、`crates/scene/src/editor/mod.rs`、`crates/scene/src/editor/select.rs`（只有 overlay 與游標的呼叫端）、`crates/app/src/edit_input.rs`（`Cursor::Grab` 對應，讓 workspace 編譯）
- Test: `crates/scene/src/transform.rs` 的單元測試

**Interfaces:**
- Consumes: 既有的 `corner_handles`、`selection_handles`、`handle_at`、`resize_offset`。
- Produces:

```rust
// scene::transform
pub enum HandleKind { N, S, E, W, Nw, Ne, Sw, Se, Rotation }

/// The selection's resize squares and its rotation handle (`getTransformHandles` /
/// `getTransformHandlesFromCoords`): each square is axis-aligned, centered where the
/// unrotated square's center lands after turning by the element's angle around its center.
pub fn selection_handles(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
    zoom: f64,
) -> SelectionHandles;

pub struct SelectionHandles {
    pub resize: Vec<(HandleKind, Bounds)>,
    pub rotation: Option<Bounds>,
}

// scene::editor
pub enum Cursor { /* 既有變體 */, Grab }
pub struct Overlay { /* 既有欄位 */, pub rotation_handle: Option<Bounds> }
```

行為（照 `transformHandles.ts`、`resizeTest.ts`）：

- `resizable_bounds` 拿掉 `angle != 0.0` 的閘門；Raw、locked、elbow arrow、單選兩點 line／arrow 仍然沒有任何控制點（也沒有旋轉控制點）。
- 單選：位置用元件的 absolute coords 與它的 `angle`。多選：共同外框、角度 0、margin 4，`OMIT_SIDES_FOR_MULTIPLE_ELEMENTS`（旋轉控制點保留）。
- 旋轉控制點位置：`x1 + width/2 - handle/2`，`y1 - dashedLineMargin - handleMargin + centeringOffset - ROTATION_RESIZE_HANDLE_GAP / zoom`（`ROTATION_RESIZE_HANDLE_GAP = 16`），再照角度旋轉中心。
- `handle_at`：先測旋轉控制點，再測角落，再測 `getSelectionBorders` 依角度旋轉後的四邊。
- 游標：`Rotation` 為 `Cursor::Grab`；縮放游標照 `getCursorForResizingElement` 的 `shouldSwapCursors` 與 `rotateResizeCursor`（依角度在 `ns`、`nesw`、`ew`、`nwse` 間輪轉）。
- `Editor::overlay` 填 `rotation_handle`；`app/edit_input.rs` 把 `Cursor::Grab` 對應到 `egui::CursorIcon::Grab`（畫旋轉控制點在 Task 3）。

- [ ] **Step 1：失敗的單元測試**

```rust
#[test]
fn a_selected_rectangle_has_a_rotation_handle_above_its_top_edge() {
    let file = sample::file(vec![sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0])]);
    let selection = Selection::from_ids(["r"]);
    let handles = selection_handles(&mut GeometryCache::default(), &file, &selection, 1.0);
    // margin 2, handle 8, centering (8 - 4) / 2 = 2: top = 0 - 2 - 8 + 2 = -8, minus the gap 16.
    assert_eq!(handles.rotation, Some([46.0, -24.0, 54.0, -16.0]));
    assert_eq!(handles.resize.len(), 4);
}

#[test]
fn a_rotated_rectangle_keeps_its_handles_and_they_turn_with_it() {
    let file = sample::file(vec![sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
        json!({"angle": std::f64::consts::FRAC_PI_2}),
    )]);
    let selection = Selection::from_ids(["r"]);
    let mut geometry = GeometryCache::default();
    let handles = selection_handles(&mut geometry, &file, &selection, 1.0);
    let rotation = handles.rotation.expect("rotation handle");
    // The unrotated handle center (50, -20) turns a quarter around (50, 25) to (95, 25).
    let center = [(rotation[0] + rotation[2]) / 2.0, (rotation[1] + rotation[3]) / 2.0];
    assert!((center[0] - 95.0).abs() < 1e-9 && (center[1] - 25.0).abs() < 1e-9, "{center:?}");
    assert_eq!(handle_at(&mut geometry, &file, &selection, [95.0, 25.0], 1.0), Some(HandleKind::Rotation));
    // The local east side now faces down; its border, padded by SIDE_RESIZING_THRESHOLD (4),
    // runs along y = 79, and (50, 77) is within that threshold of it.
    assert_eq!(handle_at(&mut geometry, &file, &selection, [50.0, 77.0], 1.0), Some(HandleKind::E));
}

#[test]
fn two_point_arrows_and_raw_elements_have_no_rotation_handle() {
    let file = sample::file(vec![
        sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [100.0, 40.0]]),
        json!({"id": "img", "type": "frame", "x": 200, "y": 0, "width": 50, "height": 50}),
    ]);
    let mut geometry = GeometryCache::default();
    assert_eq!(selection_handles(&mut geometry, &file, &Selection::from_ids(["a"]), 1.0).rotation, None);
    assert_eq!(selection_handles(&mut geometry, &file, &Selection::from_ids(["img"]), 1.0).rotation, None);
}
```

（`Selection` 的建構方式、`GeometryCache` 的建立照既有測試；`frame` 在這裡讀成 `Element::Raw`。若 `sample::file` 對 Raw 元件需要更多欄位，照既有 Raw 測試補齊。）

Run: `cargo test -p scene --lib transform::tests`
Expected: FAIL。

- [ ] **Step 2：實作，既有控制點測試照新型別調整，全部檢查並 commit**

```bash
git add crates
git commit -m "Show handles on rotated elements and add the rotation handle"
```

---

### Task 3：旋轉手勢與畫旋轉控制點

**Files:**
- Modify: `crates/scene/src/editor/select.rs`、`crates/scene/src/editor/mod.rs`、`crates/app/src/overlay.rs`
- Test: `crates/scene/tests/editor_select.rs`

**Interfaces:**
- Consumes: Task 1 的 `rotate_elements`；Task 2 的 `HandleKind::Rotation`、`Overlay::rotation_handle`。
- Produces: 選取工具的 `Gesture::Rotate(RotateState { start, selection_before, targets, center })`。

行為：

- `pointer_down` 命中 `HandleKind::Rotation` 時開始旋轉手勢（點的判定、`hit_point` 的順序與縮放相同），`center` 是按下時目標的中心：單選用元件 absolute coords 的中心，多選用 `selected_bounds` 的中心。
- `pointer_move` 每次從 `start` 呼叫 `rotate_elements(.., event.at, center, event.modifiers.shift, ..)`；`pointer_up` 與 `finish_gesture` 結束成一步 undo，選取不變。
- 旋轉中游標維持 `Cursor::Grab`。
- `app/src/overlay.rs`：`rotation_handle` 畫成與縮放方塊同色、同線寬的圓（外切於該方塊），在縮放方塊之後畫。

- [ ] **Step 1：失敗的測試**（`crates/scene/tests/editor_select.rs`）

```rust
#[test]
fn dragging_the_rotation_handle_rotates_in_one_undo_step() {
    let mut e = editor(vec![solid("r", [0.0, 0.0, 100.0, 50.0])]);
    click(&mut e, at(50.0, 25.0));
    assert!(e.overlay(1.0).rotation_handle.is_some());
    e.pointer_move(at(50.0, -20.0), &mut CharWidthMeasure);
    assert_eq!(e.cursor(), Cursor::Grab);
    drag(&mut e, at(50.0, -20.0), [100.0, 25.0]);
    let p = element(&e, "r").placement().unwrap();
    assert!((p.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12, "{}", p.angle);
    assert_eq!(rect_of(&e, "r"), [0.0, 0.0, 100.0, 50.0]);
    assert_eq!(selected(&e), ["r"]);
    assert!(e.command(Command::Undo));
    assert_eq!(element(&e, "r").placement().unwrap().angle, 0.0);
    assert!(!e.command(Command::Undo));
}

#[test]
fn shift_while_rotating_snaps_to_fifteen_degrees() {
    let mut e = editor(vec![solid("r", [0.0, 0.0, 100.0, 50.0])]);
    click(&mut e, at(50.0, 25.0));
    e.pointer_down(at(50.0, -20.0), &mut CharWidthMeasure);
    e.pointer_move(shift(100.0, 30.0), &mut CharWidthMeasure);
    e.pointer_up(shift(100.0, 30.0), &mut CharWidthMeasure);
    let angle = element(&e, "r").placement().unwrap().angle;
    assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{angle}");
}

#[test]
fn a_rotated_rectangle_resizes_along_its_own_axis() {
    let mut e = editor(vec![sample::with(
        solid("r", [0.0, 0.0, 100.0, 50.0]),
        json!({"angle": std::f64::consts::FRAC_PI_2}),
    )]);
    click(&mut e, at(50.0, 25.0));
    // Its local east side faces down now (padded border at y = 79). Grabbing it at (50, 77)
    // and pulling 20 further down widens it to 120 while the west side (at y = -25) stays put,
    // so the center moves from (50, 25) to (50, 35).
    drag(&mut e, at(50.0, 77.0), [50.0, 97.0]);
    let [x, y, w, h] = rect_of(&e, "r");
    assert!((w - 120.0).abs() < 1e-9 && (h - 50.0).abs() < 1e-9, "{w} {h}");
    assert!((x + 10.0).abs() < 1e-9 && (y - 10.0).abs() < 1e-9, "{x} {y}");
}

#[test]
fn rotating_a_multi_selection_turns_it_around_the_common_center() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 40.0, 20.0]),
        solid("b", [60.0, 0.0, 40.0, 20.0]),
    ]);
    e.command(Command::SelectAll);
    // Common bounds [0, 0, 100, 20], margin 4: the rotation handle is centered at (50, -22).
    drag(&mut e, at(50.0, -22.0), [100.0, 10.0]);
    let [ax, ay, ..] = rect_of(&e, "a");
    let [bx, by, ..] = rect_of(&e, "b");
    assert!((ax - 30.0).abs() < 1e-9 && (ay + 30.0).abs() < 1e-9, "{ax} {ay}");
    assert!((bx - 30.0).abs() < 1e-9 && (by - 30.0).abs() < 1e-9, "{bx} {by}");
}

#[test]
fn a_two_point_arrow_has_no_rotation_handle() {
    let mut e = editor(vec![sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [100.0, 40.0]])]);
    click(&mut e, at(50.0, 20.0));
    assert_eq!(e.overlay(1.0).rotation_handle, None);
}
```

（`solid`、`click`、`drag`、`shift` 等是同檔與 `support` 既有的 helper；`pointer_down`／`pointer_move`／`pointer_up` 的量字器參數照 M6a 之後的簽名。點中旋轉過的元件的座標若因碰撞判定落空，改點元件中心附近，斷言的數值不變。）

Run: `cargo test -p scene --test editor_select`
Expected: FAIL。

- [ ] **Step 2：實作 scene 端，測試通過；`app/src/overlay.rs` 畫旋轉控制點並加一個 `overlay::shapes` 的單元測試（有 `rotation_handle` 時輸出一個 `egui::Shape::Circle`）。**

- [ ] **Step 3：GUI 煙霧測試**

暫時 `HOME` 下放一個 `.excalidraw`，內含一個 `angle: 0.5` 的矩形與一個 `angle: 0.5`、帶標籤的橢圓，啟動 release build、截圖，確認兩者照角度畫出、標籤跟著轉，`kill -TERM` 關閉。（選取與旋轉控制點需要滑鼠操作，留給人工驗收。）

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates
git commit -m "Rotate the selection by dragging its rotation handle"
```

- [ ] **Step 5：回報人工驗收清單**（使用者本人操作）

1. 選一個矩形：選取框上方中間有一個圓形控制點，游標移上去變成抓取手勢。
2. 拖動圓形控制點：矩形繞中心旋轉；按住 Shift 時每 15° 一格。`Ctrl+Z` 一次回到原角度。
3. 旋轉過的矩形：四個角落控制點跟著轉，拖角落或邊會沿矩形自己的方向縮放。
4. 帶標籤的框旋轉後，標籤跟著轉、仍然置中；拖邊縮窄時標籤重新換行。
5. 框選兩三個元件一起旋轉：整組繞共同中心轉；其中有旋轉過的元件時，拖角落會等比例縮放。
6. 選一支只有兩個點的箭頭：沒有旋轉控制點。
7. 存檔後丟進 excalidraw.com：角度、位置與 napkin 相同。

---

## 自我檢查

- 旋轉 spec §3「控制點」：Task 2（位置、命中、游標）、Task 3（手勢、Shift、undo、畫圓）。
- 「哪些元件可以旋轉」：Task 1 的單元測試與基準涵蓋 rectangle、ellipse、diamond、text、freedraw、多點 line、帶標籤容器；Task 2 測兩點箭頭與 Raw 沒有旋轉控制點。image 的旋轉在 M6d 做出 image 元件後自然適用。
- 「旋轉後的縮放」與取消 M4a 決定 10：Task 1 的 `keepAspectRatio` 條件與基準、Task 2 拿掉閘門、Task 3 的編輯器測試。
- 「正確性」：Task 1 的 `transform` 基準群組。
- 「不做」：沒有屬性面板旋轉欄位、鍵盤旋轉、crop 的任務。
