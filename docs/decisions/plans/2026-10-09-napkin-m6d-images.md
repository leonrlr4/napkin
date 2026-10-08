# napkin M6d：圖片 Implementation Plan

> Historical record, frozen 2026-10-09. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Ctrl+V` 貼上剪貼簿裡的圖片、從檔案管理員拖放圖片檔，圖片存成 Excalidraw 的 `image` 元件與 `files`，畫在畫布上，能搬移、縮放、旋轉、刪除、複製貼上，並與 excalidraw.com 互通。

**Architecture:** `scene` 只管資料：型別化的 `Element::Image`、`SceneFile` 的 `files` 讀寫、`newImageElement`、插入位置與尺寸（`getImageNaturalDimensions`、`positionElementsOnGrid`）、剪貼簿 JSON 帶 `files`、圖片的縮放規則（預設等比例、控制點 margin 0）。`app` 管位元組：新模組 `image_file` 做格式辨識、SHA-1、超過 1440 px 縮小、4 MiB 上限、data URL；新模組 `clipboard_image` 用 `wl-clipboard-rs` 讀剪貼簿；既有的 `wl_keyboard`（`pinch.rs`）偵測 `Ctrl+V`；渲染端新增 `ImageStore`（背景解碼、以 `fileId` 快取）與 `DrawItem::Image`（沿用旋轉文字已有的貼圖管線畫一個旋轉過的貼圖四邊形）。

**Tech Stack:** Rust 1.98.1、eframe／egui 0.36.2、wgpu 30。新依賴只給 `app`：`wl-clipboard-rs`、`image`（`default-features = false`，只開 `png`、`jpeg`、`webp`、`gif`）、`sha1`、`base64`。

**Spec:** `docs/decisions/specs/2026-10-09-napkin-rotation-images-design.md` §4（M6d）。

**前置：** 分支 `m6d-images`，從 `master` 的 `028edc7`（M6c 已合併）開出，在原本的 checkout 工作，不開 worktree。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過，不提任務編號或計畫的決定編號（可以引用「spec §N」與 Excalidraw 函數名稱）。module doc 提到 Excalidraw 版本時寫「the pinned commit」，不要再複製 commit hash。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 行為以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`。port 時看 JS 原始碼，不看計畫的摘要；兩者不一致時照 JS，並在回報寫出差異。
- `scene` 與 `rough` 不能依賴 egui、wgpu、glyphon，也不新增依賴；`scene` 不解碼、不雜湊圖片位元組。
- JS 數值語意走 `rough::js`；亂數與時間走 `scene::env::Env`。
- 每次修改元件都呼叫 `scene::new_element::bump_version`，而且只在值真的改變時呼叫。
- 元件只在能無損寫回時才用型別化結構（`Element::from_value` 的 round-trip 檢查）；新增欄位時 `Slot<T>` 與 `Option<T>` 的選擇照 CLAUDE.md，`crates/scene/tests/corpus.rs` 的「型別化元件不退回 Raw」檢查要涵蓋 image。
- 每個會改元件的使用者動作是一步 undo（主 spec §5.7）。插入圖片（含多張）是一步。
- `files` 不進 undo：undo 插入圖片只移除元件，`files` 裡的資料留著（Excalidraw 也不立即刪）。
- 不支援 SVG：貼上或拖放 SVG 時顯示通知「不支援 SVG 圖片」，不建立元件。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`，並把三個指令的實際輸出尾段貼進回報。GPU 測試沒有 adapter 時直接失敗，不跳過。改了 `tools/baseline/scene/cases.mjs` 或 `generate.mjs` 的任務要執行 `cd tools/baseline && npm run scene` 並 commit 產出的 JSON，其他群組的 JSON 必須完全不變。
- `skills/napkin/SKILL.md` 描述 AI 介面：改 `crates/app/src/control/summary.rs` 的摘要格式或 `scene::batch` 接受的欄位時一起改它。
- GUI 煙霧測試只做開視窗、截圖、關閉；用自己啟動的行程 PID 找視窗，`kill -TERM <pid>` 關閉；保留真實的 `WAYLAND_DISPLAY`／`XDG_RUNTIME_DIR`，只換 `HOME`；視窗只開幾秒。不用 `wtype`，不送任何按鍵或滑鼠事件，不讀寫使用者的剪貼簿。

## 寫計畫時做的決定

1. **`Ctrl+V` 由 `wl_keyboard` 偵測。** egui-winit 0.36 把 `Ctrl+V` 直接換成 `Event::Paste(text)`，剪貼簿沒有文字時什麼事件都不送（`on_keyboard_input` 在送出 `Key` 事件前就 `return`）。`pinch.rs` 已經在 eframe 的 Wayland 連線上綁了一個 `wl_keyboard`（用來追蹤 Super 鍵），擴充它在 Ctrl 按住時看到 V（evdev keycode 47）按下就送出一個「貼上要求」。
2. **畫布的貼上一律走 `wl-clipboard-rs`。** 收到貼上要求、而且沒有 egui 元件拿著鍵盤（文字編輯框自己用 egui 的貼上）時，背景執行緒列出剪貼簿的 MIME 類型：有 `image/png`、`image/jpeg`、`image/webp`、`image/gif` 就照這個順序讀第一個當圖片（`insertClipboardContent` 圖片優先於元件 JSON 與文字）；有 `image/svg+xml` 而沒有其他圖片就顯示不支援 SVG；否則讀 `text/plain;charset=utf-8`（或 `text/plain`、`UTF8_STRING`）走既有的 `Editor::paste`。這時畫布忽略 egui 的 `Event::Paste`，同一次按鍵才不會貼兩次。`wl-clipboard-rs` 無法使用（compositor 沒有 data-control）時退回現在的 egui `Event::Paste` 文字貼上，log 一行。
3. **插入不經過佔位元件。** Excalidraw 先放一個 `status: "pending"` 的佔位元件再非同步初始化；napkin 先在背景把位元組處理完（解碼尺寸、縮小、雜湊、data URL），完成後才一次插入 `status: "saved"` 的元件，位置與尺寸照 `newImagePlaceholder` 加 `getImageNaturalDimensions` 算出的最終結果。效果相同，少一個中間狀態。
4. **`fileId` 是原始位元組的 SHA-1**（縮小之前，`generateIdFromFile`）。`files` 已有同一個 `fileId` 且有 `dataURL` 時不重新處理位元組。
5. **縮小**照 `resizeImageFile`：長邊超過 1440 px 時等比例縮成長邊 1440（`image` crate 的 `Lanczos3`），用原格式重新編碼（jpeg 品質 92；gif、webp 編碼器不可用時改存 PNG，`mimeType` 跟著改）。縮小後超過 4 MiB 就拒絕並通知。
6. **AI 介面。** 摘要裡的圖片元件輸出 `image` 類型、`x y w h`、`angle`，不輸出 `fileId`。batch 不能新增圖片；`update` 對圖片接受 `x`、`y`、`width`、`height`（維持現在 Raw 只能搬移的寬鬆版本），`delete` 照舊。

## 檔案結構

```
crates/scene/src/
  element.rs        Element::Image(ImageElement)
  file.rs           files 讀寫
  new_element.rs    new_image_element
  image.rs          插入：placeholder 位置、natural dimensions、grid、Editor::insert_images 的規則
  clipboard.rs      剪貼簿 JSON 帶 files
  transform.rs      圖片控制點 margin 0、預設等比例
  editor/mod.rs     insert_images、paste 帶 files
  batch/、shape/、collision.rs、geometry.rs、selection.rs   Image 變體的對應分支
crates/app/src/
  image_file.rs     格式辨識、SHA-1、縮小、4 MiB、data URL
  clipboard_image.rs  wl-clipboard-rs 讀取（背景執行緒）
  pinch.rs          Ctrl+V 偵測
  render/image_store.rs  背景解碼、以 fileId 快取
  render/plan.rs、render/gpu.rs   DrawItem::Image
  napkin_app.rs     接線：貼上、拖放、通知
  control/summary.rs
skills/napkin/SKILL.md
```

---

### Task 1：`Element::Image`、`files`、`new_image_element` 與圖片的編輯規則

**Files:**
- Modify: `crates/scene/src/element.rs`、`crates/scene/src/file.rs`、`crates/scene/src/new_element.rs`、`crates/scene/src/transform.rs`、`crates/scene/src/editor/select.rs`、`crates/scene/tests/corpus.rs`，以及編譯器要求補上 `Element::Image` 分支的每個 `match`（`collision.rs`、`geometry.rs`、`selection.rs`、`shape/`、`batch/`、`duplicate.rs`、`edit.rs`、`app` 端的 `render/plan.rs`、`control/summary.rs` 等）；`tools/baseline/scene/cases.mjs`（`new_element` 群組加 `newImageElement` 案例）；`skills/napkin/SKILL.md`
- Regenerate: `crates/scene/tests/baseline/new_element.json`

**Interfaces:**
- Produces:

```rust
// scene::element
pub enum Element { /* 既有變體 */, Image(ImageElement) }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub file_id: Slot<String>,   // Excalidraw writes null before a file is attached
    pub status: String,          // "pending" | "saved" | "error"
    pub scale: [f64; 2],
    pub crop: Slot<ImageCrop>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCrop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub natural_width: f64,
    pub natural_height: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// scene::file
#[derive(Clone, Debug, PartialEq)]
pub struct FileData {
    pub id: String,
    pub mime_type: String,
    pub data_url: String,
    pub created: f64,
    pub last_retrieved: f64,
}
impl SceneFile {
    /// `files[id].dataURL` and `mimeType`, if present.
    pub fn file_data(&self, id: &str) -> Option<FileData>;
    /// Adds `data` under `files[data.id]` unless an entry with a `dataURL` already exists
    /// (`addMissingFiles`). Returns whether it added one.
    pub fn add_missing_file(&mut self, data: FileData) -> bool;
}

// scene::new_element
pub fn new_image_element(
    props: ElementProps,
    image: ImageProps,
    env: &mut impl Env,
) -> Element;
pub struct ImageProps {
    pub file_id: Option<String>,
    pub status: Option<String>,
    pub scale: Option<[f64; 2]>,
}
```

行為：

- 欄位與預設值照 `newImageElement`（`strokeColor` 固定 `"transparent"`、`status` 預設 `"pending"`、`fileId` 預設 `null`、`scale` 預設 `[1, 1]`、`crop` 預設 `null`）。實際的 key 集合以 `new_element` 基準群組的 JS 輸出為準。
- 現在對 image 走 `Raw` 的地方（碰撞判定當矩形、外框、刪除、搬移、複製一份、橡皮擦）改走 `Image`，行為不變；`Raw` 對 image 的特殊處理若只剩死碼就刪掉。
- 控制點：`getTransformHandles` 對 image 的 margin 是 0、spacing 是 0；`resizeTest` 的側邊 `SPACING` 對 image 是 0。
- 縮放：選取裡有 image 時預設等比例，Shift 反轉（`App.tsx` 的 `proportionalByDefault`）。`resizeSingleElement` 對 image 的 `scale` 翻轉照 JS（拉過對邊時 `scale` 的正負號跟著變）。
- 渲染端這個任務只讓 `app` 編譯：`Element::Image` 仍畫成現在的虛線佔位框加類型名稱（Task 5 才畫圖）。
- AI：`summary.rs` 的圖片行照決定 6；`batch` 的 `update` 對 image 接受 `x`、`y`、`width`、`height`；`SKILL.md` 的對應段落一起改。

- [ ] **Step 1：失敗的測試**

`crates/scene/src/element.rs` 的測試：

```rust
#[test]
fn an_excalidraw_image_round_trips_as_a_typed_image() {
    let value = json!({
        "id": "i", "type": "image", "x": 10, "y": 20, "width": 300, "height": 200,
        "angle": 0, "strokeColor": "transparent", "backgroundColor": "transparent",
        "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
        "opacity": 100, "groupIds": [], "frameId": null, "index": "a0", "roundness": null,
        "seed": 1, "version": 3, "versionNonce": 7, "isDeleted": false, "boundElements": null,
        "updated": 1, "link": null, "locked": false,
        "status": "saved", "fileId": "abc", "scale": [1, 1], "crop": null
    });
    let element = Element::from_value(value.clone());
    assert!(matches!(element, Element::Image(_)), "{element:?}");
    assert_eq!(element.to_value(), value);
}
```

`crates/scene/src/file.rs` 的測試：

```rust
#[test]
fn files_are_read_and_added_without_overwriting() {
    let mut file = SceneFile::new();
    let data = FileData {
        id: "f1".into(),
        mime_type: "image/png".into(),
        data_url: "data:image/png;base64,AAAA".into(),
        created: 1.0,
        last_retrieved: 1.0,
    };
    assert!(file.add_missing_file(data.clone()));
    assert!(!file.add_missing_file(FileData { data_url: "data:image/png;base64,BBBB".into(), ..data }));
    assert_eq!(file.file_data("f1").unwrap().data_url, "data:image/png;base64,AAAA");
    let written: Value = serde_json::from_str(&file.to_json_string()).unwrap();
    assert_eq!(written["files"]["f1"]["mimeType"], json!("image/png"));
    assert_eq!(written["files"]["f1"]["lastRetrieved"], json!(1.0));
}
```

`crates/scene/tests/corpus.rs`：把 image 加進「型別化元件在語料裡不會退回 Raw」的類型清單（語料 `image.excalidraw` 已有圖片）。

`crates/scene/tests/editor_select.rs`：

```rust
#[test]
fn an_image_resizes_proportionally_by_default_and_freely_with_shift() {
    let image = json!({
        "id": "i", "type": "image", "x": 0, "y": 0, "width": 200, "height": 100,
        "angle": 0, "strokeColor": "transparent", "backgroundColor": "transparent",
        "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
        "opacity": 100, "groupIds": [], "frameId": null, "index": "a0", "roundness": null,
        "seed": 1, "version": 1, "versionNonce": 1, "isDeleted": false, "boundElements": null,
        "updated": 1, "link": null, "locked": false,
        "status": "saved", "fileId": "f", "scale": [1, 1], "crop": null
    });
    let mut e = editor(vec![image.clone()]);
    click(&mut e, at(100.0, 50.0));
    // Image handles have no margin: the se square's center sits at the corner (200, 100).
    drag(&mut e, at(200.0, 100.0), [400.0, 120.0]);
    let [_, _, w, h] = rect_of(&e, "i");
    assert!((w / h - 2.0).abs() < 1e-9, "{w} {h}");

    let mut e = editor(vec![image]);
    click(&mut e, at(100.0, 50.0));
    drag(&mut e, shift(200.0, 100.0), [400.0, 120.0]);
    let [_, _, w, h] = rect_of(&e, "i");
    assert!((w / h - 2.0).abs() > 0.1, "{w} {h}");
}
```

（se 方塊中心的確切位置照移植後的 `getTransformHandlesFromCoords` 在 margin 0、spacing 0 時算；若不在 (200, 100)，改用算出的位置，斷言不變。`shift` 按下時若被當成加選，改成按下不帶 Shift、移動與放開帶 Shift。）

Run: `cargo test -p scene`
Expected: 編譯失敗（`Element::Image` 不存在）。

- [ ] **Step 2：實作；`new_element` 基準加 `newImageElement` 案例（一個只給必要欄位、一個帶 `fileId`／`status`／`scale`），重新產生，全部檢查並 commit**

```bash
git add crates skills tools/baseline/scene
git commit -m "Type image elements and their files"
```

---

### Task 2：插入圖片與帶 `files` 的剪貼簿

**Files:**
- Create: `crates/scene/src/image.rs`
- Modify: `crates/scene/src/lib.rs`、`crates/scene/src/clipboard.rs`、`crates/scene/src/editor/mod.rs`、`crates/scene/src/duplicate.rs`（若貼上路徑需要）
- Test: `crates/scene/tests/editor_images.rs`（新檔）、`crates/scene/src/clipboard.rs` 的測試

**Interfaces:**
- Consumes: Task 1 的 `ImageElement`、`FileData`、`new_image_element`、`SceneFile::add_missing_file`。
- Produces:

```rust
// scene::image
/// One prepared image the app hands to the editor: everything `initializeImage` derives from
/// the file before the element exists.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImage {
    pub file: FileData,
    pub natural_size: [f64; 2],
}

/// `newImagePlaceholder` + `getImageNaturalDimensions`: the element's `[x, y, width, height]`
/// for an image of `natural_size` inserted centered on `at`, given the canvas's height in
/// screen points and the zoom.
pub fn natural_placement(at: [f64; 2], natural_size: [f64; 2], canvas_height: f64, zoom: f64) -> [f64; 4];

/// `positionElementsOnGrid` for single-element units.
pub fn position_on_grid(placements: &[[f64; 4]], center: [f64; 2], padding: f64) -> Vec<[f64; 4]>;

impl<E: Env> Editor<E> {
    /// `insertImages` with every file already prepared: adds the files, creates one saved
    /// image per entry laid out on the grid around `at` (`gridPadding = 50 / zoom`), selects
    /// them, switches to the selection tool. One history step; `false` when `images` is empty
    /// or the editor is not idle.
    pub fn insert_images(&mut self, images: Vec<PreparedImage>, at: [f64; 2], canvas_height: f64, zoom: f64) -> bool;
}
```

行為：

- `natural_placement`：佔位框 `100 / zoom` 見方、以 `at` 為中心；`minHeight = max(canvas_height - 120, 160)`、`maxHeight = min(minHeight, floor(canvas_height * 0.5) / zoom)`、`height = min(naturalHeight, maxHeight)`、`width = height * naturalWidth / naturalHeight`，再以佔位框中心置中（照 JS 的 `getImageNaturalDimensions`，以 JS 為準）。
- `clipboard::serialize`：選取裡有 image 時，`files` 帶上它們 `fileId` 對應的資料（`serializeAsClipboardJSON`）。`clipboard::parse` 回傳的結果多一個 `files`；`Editor::paste` 把其中元件引用到的檔案用 `add_missing_file` 加進場景。
- 複製一份（`Ctrl+D`）沿用同一個 `fileId`，不需要改 `files`。

要讀的 JS：`App.tsx` 的 `insertImages`、`newImagePlaceholder`、`getImageNaturalDimensions`、`addMissingFiles`；`positionElementsOnGrid.ts`；`clipboard.ts` 的 `serializeAsClipboardJSON`、`parseClipboard`。

- [ ] **Step 1：失敗的測試**

`crates/scene/tests/editor_images.rs`：

```rust
mod support;

use scene::editor::{Command, Tool};
use scene::file::FileData;
use scene::image::{PreparedImage, natural_placement};
use serde_json::json;
use support::*;

fn png(id: &str, natural: [f64; 2]) -> PreparedImage {
    PreparedImage {
        file: FileData {
            id: id.into(),
            mime_type: "image/png".into(),
            data_url: "data:image/png;base64,AAAA".into(),
            created: 1.0,
            last_retrieved: 1.0,
        },
        natural_size: natural,
    }
}

#[test]
fn natural_placement_fits_tall_images_to_half_the_canvas() {
    // Canvas 900 tall at zoom 1: min(780, 450) = 450 is the height cap.
    assert_eq!(natural_placement([500.0, 500.0], [400.0, 300.0], 900.0, 1.0), [300.0, 350.0, 400.0, 300.0]);
    assert_eq!(natural_placement([500.0, 500.0], [2000.0, 1000.0], 900.0, 1.0), [50.0, 275.0, 900.0, 450.0]);
}

#[test]
fn inserting_an_image_adds_its_file_selects_it_and_undoes_in_one_step() {
    let mut e = editor(vec![]);
    assert!(e.insert_images(vec![png("f1", [400.0, 300.0])], [500.0, 500.0], 900.0, 1.0));
    let v = e.file().elements[0].to_value();
    assert_eq!(v["type"], json!("image"));
    assert_eq!(v["fileId"], json!("f1"));
    assert_eq!(v["status"], json!("saved"));
    assert_eq!([v["x"].clone(), v["y"].clone(), v["width"].clone(), v["height"].clone()],
        [json!(300.0), json!(350.0), json!(400.0), json!(300.0)]);
    assert!(e.file().file_data("f1").is_some());
    assert_eq!(selected(&e), [v["id"].as_str().unwrap()]);
    assert_eq!(e.tool(), Tool::Selection);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
    assert!(e.file().file_data("f1").is_some(), "files are not part of undo");
}

#[test]
fn copying_an_image_carries_its_file_into_the_paste() {
    let mut e = editor(vec![]);
    e.insert_images(vec![png("f1", [100.0, 100.0])], [0.0, 0.0], 900.0, 1.0);
    let text = e.copy_selection().expect("something selected");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["files"]["f1"]["dataURL"], json!("data:image/png;base64,AAAA"));

    let mut other = editor(vec![]);
    assert!(other.paste(&text, [50.0, 50.0], &mut scene::sample::CharWidthMeasure));
    assert!(other.file().file_data("f1").is_some());
}
```

Run: `cargo test -p scene --test editor_images`
Expected: 編譯失敗。

- [ ] **Step 2：實作，全部檢查並 commit**

```bash
git add crates/scene
git commit -m "Insert images and carry their files through the clipboard"
```

---

### Task 3：`image_file`：格式、SHA-1、縮小、data URL

**Files:**
- Create: `crates/app/src/image_file.rs`
- Modify: `crates/app/src/lib.rs`、`crates/app/Cargo.toml`、`Cargo.toml`（workspace 依賴）

**Interfaces:**
- Consumes: Task 2 的 `scene::image::PreparedImage`、`scene::file::FileData`。
- Produces:

```rust
// app::image_file
#[derive(Debug, PartialEq)]
pub enum PrepareError {
    Svg,
    Unsupported,
    TooBig,
    Decode(String),
}

/// `initializeImage`'s file work, off the UI thread: sniffs the format from the bytes
/// (not the claimed MIME type), rejects SVG, takes the SHA-1 of the original bytes as the
/// `fileId`, downsizes past 1440 px on the long side and re-encodes, rejects anything still
/// over 4 MiB, and builds the data URL. `now_ms` stamps `created`/`lastRetrieved`.
pub fn prepare(bytes: &[u8], now_ms: f64) -> Result<PreparedImage, PrepareError>;

/// Lowercase hex SHA-1 (`generateIdFromFile`).
pub fn file_id(bytes: &[u8]) -> String;
```

- [ ] **Step 1：失敗的測試**（`image_file.rs` 的 `#[cfg(test)]`；測試圖用 `image` crate 在記憶體裡生成）

```rust
#[test]
fn file_ids_are_sha1_hex() {
    assert_eq!(file_id(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
}

#[test]
fn a_small_png_keeps_its_bytes_and_size() {
    let bytes = encode_png(300, 200);
    let prepared = prepare(&bytes, 5.0).unwrap();
    assert_eq!(prepared.natural_size, [300.0, 200.0]);
    assert_eq!(prepared.file.id, file_id(&bytes));
    assert_eq!(prepared.file.mime_type, "image/png");
    assert_eq!(prepared.file.data_url, format!("data:image/png;base64,{}", base64_encode(&bytes)));
    assert_eq!(prepared.file.created, 5.0);
}

#[test]
fn a_large_image_is_downsized_to_1440_on_its_long_side() {
    let bytes = encode_png(3000, 1500);
    let prepared = prepare(&bytes, 0.0).unwrap();
    assert_eq!(prepared.natural_size, [1440.0, 720.0]);
    assert_eq!(prepared.file.id, file_id(&bytes), "the id hashes the original bytes");
}

#[test]
fn svg_and_garbage_are_rejected() {
    assert_eq!(prepare(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>", 0.0), Err(PrepareError::Svg));
    assert_eq!(prepare(b"not an image", 0.0), Err(PrepareError::Unsupported));
}
```

（`encode_png`、`base64_encode` 是測試內的小 helper。4 MiB 的拒絕路徑用一張無法壓縮的雜訊圖或把上限做成可注入的常數測試，擇一，在回報說明。）

Run: `cargo test -p app image_file`
Expected: 編譯失敗。

- [ ] **Step 2：實作，全部檢查並 commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "Prepare pasted and dropped image files"
```

---

### Task 4：`Ctrl+V` 與拖放接到編輯器

**Files:**
- Create: `crates/app/src/clipboard_image.rs`
- Modify: `crates/app/src/pinch.rs`、`crates/app/src/edit_input.rs`、`crates/app/src/napkin_app.rs`、`crates/app/Cargo.toml`、`Cargo.toml`

**Interfaces:**
- Consumes: Task 2 的 `Editor::insert_images`；Task 3 的 `image_file::prepare`。
- Produces:

```rust
// app::clipboard_image
pub enum ClipboardContent {
    Image(Vec<u8>),
    Svg,
    Text(String),
    Empty,
}
/// Reads the regular clipboard through wl-clipboard-rs (data-control): the first of
/// image/png, image/jpeg, image/webp, image/gif, else Svg for image/svg+xml, else UTF-8 text.
/// `Err` when data-control is unavailable.
pub fn read() -> Result<ClipboardContent, String>;
/// The MIME preference, testable without a compositor.
pub fn choose(offered: &[String]) -> Option<&'static str>;

// app::pinch: PinchListener gains
pub fn take_paste_requests(&self) -> usize;
```

行為：

- `pinch.rs` 的 `wl_keyboard` 追蹤 Ctrl 修飾鍵（`modifiers` 事件的 `mods_depressed`，Ctrl 對應的位元照 xkb 的 Control mask，值 4），Ctrl 按住時 key 47 按下就把計數加一；`take_paste_requests` 取出並歸零。
- 每幀：有貼上要求、編輯器在畫布狀態（不在文字編輯中、沒有 egui 元件拿鍵盤、畫布可寫）時，開一個背景執行緒 `clipboard_image::read()`，結果經 channel 回到 UI 執行緒：`Image` 走 `image_file::prepare`（同一個背景執行緒做完）再 `insert_images`（位置是最後的指標位置，沒有就畫面中心；`canvas_height` 是畫布高度的螢幕點數）；`Svg` 顯示通知；`Text` 走既有的 `Editor::paste`；`Empty` 什麼都不做。
- 決定 2 的退回：第一次 `read()` 回 `Err` 後記住「data-control 不可用」，之後畫布照舊用 egui 的 `Event::Paste`。可用時 `edit_input` 不再把 `Event::Paste` 轉成 `EditorInput::Paste`（加一個 `FrameInput` 欄位控制）。
- 拖放：`ui.input(|i| i.raw.dropped_files)` 裡每個有 `path` 的檔案讀成位元組（有 `bytes` 就直接用），背景 `prepare`，全部完成後一次 `insert_images`（位置是放下時的指標位置）。非圖片檔忽略；SVG 通知；`TooBig` 通知「圖片太大（上限 4 MB）」；`Decode` 通知並 log。
- 通知用既有的 `self.notice`。

- [ ] **Step 1：失敗的測試**

`clipboard_image.rs`：

```rust
#[test]
fn images_win_over_text_and_svg_is_reported() {
    let offered = |types: &[&str]| types.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    assert_eq!(choose(&offered(&["text/plain", "image/jpeg", "image/png"])), Some("image/png"));
    assert_eq!(choose(&offered(&["image/webp", "text/html"])), Some("image/webp"));
    assert_eq!(choose(&offered(&["image/svg+xml", "text/plain"])), Some("image/svg+xml"));
    assert_eq!(choose(&offered(&["text/plain;charset=utf-8"])), Some("text/plain;charset=utf-8"));
    assert_eq!(choose(&offered(&[])), None);
}
```

`pinch.rs`：把 Ctrl／V 的判斷抽成純函數並測試：

```rust
#[test]
fn ctrl_v_counts_as_a_paste_request() {
    assert!(is_paste_key(47, CONTROL_MASK));
    assert!(!is_paste_key(47, 0));
    assert!(!is_paste_key(46, CONTROL_MASK));
}
```

`edit_input.rs`：`FrameInput` 新欄位設定為「剪貼簿由 data-control 處理」時，`Event::Paste` 不產生 `EditorInput::Paste`。

Run: `cargo test -p app`
Expected: 編譯失敗。

- [ ] **Step 2：實作，全部檢查並 commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "Paste and drop images onto the canvas"
```

---

### Task 5：畫出圖片

**Files:**
- Create: `crates/app/src/render/image_store.rs`
- Modify: `crates/app/src/render/mod.rs`、`crates/app/src/render/plan.rs`、`crates/app/src/render/gpu.rs`、`crates/app/src/render/callback.rs`、`crates/app/src/napkin_app.rs`、`crates/app/src/control/render.rs`（AI 截圖走同一套）
- Test: `crates/app/tests/gpu_images.rs`（新檔）、`render/plan.rs` 的單元測試

**Interfaces:**
- Consumes: Task 1 的 `Element::Image`、`SceneFile::file_data`。
- Produces:

```rust
// app::render::image_store
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8.
    pub pixels: Vec<u8>,
}
pub enum ImageState { Loading, Ready(Arc<DecodedImage>), Failed }
/// Decodes `files` data URLs on a background thread, keyed by `fileId`; `request` is cheap to
/// call every frame and returns the current state.
pub struct ImageStore { /* ... */ }
impl ImageStore {
    pub fn request(&mut self, file_id: &str, file: &SceneFile) -> ImageState;
    /// File ids that finished decoding since the last call (the app repaints on them).
    pub fn take_finished(&mut self) -> bool;
}

// app::render::plan
pub enum DrawItem { /* 既有變體 */, Image(ImageDraw) }
pub struct ImageDraw { pub element: usize, pub file_id: String, pub alpha: f32 }
```

行為（照 `renderElement.ts` 的 image 分支）：

- `plan_frame`：`Element::Image` 有 `fileId` 且 `ImageStore` 回 `Ready` 時產生 `DrawItem::Image`，依檔案順序與圖形、文字交錯；`Loading` 時什麼都不畫並要求重繪；`Failed` 或沒有檔案資料時畫現在的虛線佔位框。`View` 多一個解碼結果的查詢介面。
- GPU：每個 `fileId` 一張 texture（`Rgba8Unorm`、premultiplied，沿用 `textured` 管線），快取在 `CanvasRenderer`，這一幀沒用到的 texture 在超過上限時才淘汰（上限 64 張）。四邊形照元件的 `x y width height angle` 旋轉；`scale` 為負時水平或垂直翻轉 UV；`crop` 不為 `null` 時 UV 取 `crop.x / naturalWidth` 等比例；`alpha` 套 opacity 與橡皮擦待刪淡化。深色模式不反轉。
- `control/render.rs` 的離屏渲染用同一個 `ImageStore` 的同步版本（離屏一次畫完，先同步解碼需要的檔案）。

- [ ] **Step 1：失敗的測試**

`render/plan.rs` 的單元測試：有 `Ready` 圖片的 image 元件產生 `DrawItem::Image`，在它前後的矩形各在自己的 `Meshes` 批次；`Failed` 時產生佔位框的繪製。

`crates/app/tests/gpu_images.rs`（照 `gpu_shapes.rs` 的離屏寫法）：場景只有一張 2×2 的 PNG（左上紅、右上綠、左下藍、右下白），放在 `[0, 0, 100, 100]`，渲染後讀回像素：四個象限中心分別是紅、綠、藍、白（容差 2）；同一張圖 `angle: π` 時象限顛倒；`scale: [-1, 1]` 時左右顛倒；`opacity: 50` 時紅色象限在白底上是 `(255, 128, 128)` 左右（容差 3）。

Run: `cargo test -p app --test gpu_images`
Expected: FAIL。

- [ ] **Step 2：實作，測試通過**

- [ ] **Step 3：GUI 煙霧測試**

暫時 `HOME` 下放一個 `.excalidraw`：兩張圖片元件（其中一張 `angle: 0.4`）與對應的 `files`（小 PNG data URL），加一個 `fileId` 在 `files` 裡找不到的圖片元件。啟動 release build、截圖：兩張圖照角度畫出，缺檔的那個是虛線佔位框；`kill -TERM` 關閉。

- [ ] **Step 4：全部檢查並 commit**

```bash
git add crates/app
git commit -m "Draw image elements"
```

- [ ] **Step 5：回報人工驗收清單**（使用者本人操作）

1. 用截圖工具截一張圖（複製到剪貼簿），在 napkin 按 `Ctrl+V`：圖片出現在游標位置、被選取，`Ctrl+Z` 一次移除。
2. 從瀏覽器複製一張圖片再 `Ctrl+V`：同上。剪貼簿只有文字時 `Ctrl+V` 照舊貼成文字；從 excalidraw.com 複製的元件照舊貼成元件。
3. 從檔案管理員拖一張照片（大於 1440 px）進來：圖片出現，長邊不超過畫布高度一半；拖兩三張一次放下，排成格狀。
4. 拖一個 SVG 檔進來：顯示「不支援 SVG 圖片」。
5. 圖片可以搬移、拖角落等比例縮放（按 Shift 自由縮放）、用旋轉控制點旋轉、刪除；複製一份與複製貼上都正常。
6. 存檔後把檔案拖進 excalidraw.com：圖片都在，位置、大小、角度相同；在 excalidraw.com 複製一張圖片元件再貼回 napkin：圖片顯示正常。
7. 請 Claude 用 `napkin` 看畫布：摘要列出圖片，截圖裡看得到圖片內容。

---

## 自我檢查

- spec §4「來源」：Task 4（`Ctrl+V` 經 `wl_keyboard` 與 `wl-clipboard-rs`、MIME 優先順序、拖放、位置、退回）。
- 「儲存」：Task 1（型別化元件、`files`）、Task 2（`addMissingFiles`、剪貼簿帶 `files`）、Task 3（SHA-1、1440 px、4 MiB、data URL）。
- 「大小」：Task 2 的 `natural_placement` 與一步 undo。
- 「縮放與旋轉」：Task 1 的預設等比例與 margin 0；旋轉由 M6c 提供，Task 1 只確認 image 進入控制點的可旋轉清單。
- 「渲染」：Task 5（背景解碼、交錯繪製、opacity／angle／crop／scale、缺檔佔位框、AI 截圖）。
- 「不做」：SVG 只通知；沒有 crop 工具、複製成 PNG、AI 新增圖片、動畫 GIF 的任務。
- 「新依賴」：Task 3、Task 4，都只在 `app`。
