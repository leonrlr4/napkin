# napkin M3：`app` 唯讀檢視器 Implementation Plan

> Historical record, frozen 2026-09-16. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 `app` crate（binary `napkin`）：開啟一個 `.excalidraw` 檔，用 wgpu 畫出和 excalidraw.com 一致的線條、填色與文字，可以平移、縮放（含觸控板捏合），1000 個元件的場景連續平移縮放時不掉幀。

**Architecture:** eframe 視窗裡的畫布是一個 egui paint callback，和 egui 共用同一個 4× MSAA、帶 8 bit stencil 的 render pass。每個元件依 `(id, version, 深淺色, 透明度)` 產生 rough 形狀，再依縮放級距用 lyon 三角化，頂點以場景座標存進共用的 vertex／index buffer，每幀只更新相機 uniform。半透明元件和帶標籤的箭頭用 stencil 保證同一元件的每個像素只寫一次；文字用 glyphon，照圖層順序和圖形交錯繪製，旋轉過的文字先畫到離屏 texture 再以旋轉的四邊形合成。

**Tech Stack:** Rust 1.98.1（edition 2024）、eframe／egui／egui-wgpu 0.36.2、wgpu 30.0.1、glyphon 0.12.0（cosmic-text 0.19.0）、lyon 1.0.19、bytemuck 1、toml 0.8、serde_json 1（workspace 既有）、pollster 0.4（測試）；Task 9 另加 wayland-client 0.31.15、wayland-backend 0.3.17、wayland-protocols 0.32.13、rustix 1、raw-window-handle 0.6。M1 的 `rough`、M2 的 `scene`。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（§4.1、§4.2 `app`、§5.2、§6、§6.7、§7.1 平移與縮放、§7.6、§8、§9.3 #3）
**Roadmap:** `docs/decisions/plans/2026-09-13-napkin-roadmap.md`
**M0 結論：** `docs/decisions/napkin-m0-findings.md`；探測程式在 commit `5a092f6`（`spikes/m0/src/bin/canvas_probe.rs`）與 `7e14330`（`spikes/m0/src/bin/pinch_probe.rs`），用 `git show <commit>:<path>` 取出。
**前置：** M2 已合併進 `master`（`9ff0d9e`）。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。註解描述現況，不寫變更經過。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- 繪製規則以 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 為準，原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`（不存在時執行 `cd tools/baseline && npm ci && npm run scene`）。
- `scene` 與 `rough` 不能依賴 egui、wgpu 或任何繪圖 crate。`app` 允許的依賴只有 Tech Stack 列出的套件加上 workspace 內的 `scene`、`rough`，版本照列；eframe 用 `default-features = false, features = ["default_fonts", "wayland", "wgpu"]`。
- 所有在 egui render pass 裡畫的 pipeline 與 glyphon `TextRenderer`：sample count `4`、depth-stencil 格式 `wgpu::TextureFormat::Stencil8`。eframe 的 `NativeOptions` 設 `multisampling: 4`、`stencil_buffer: 8`。
- 視窗 app_id 是 `napkin`（Hyprland 視窗規則比對它，spec §4.1）。
- M3 不寫入任何檔案：不存 `.excalidraw`，不存視角。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。GPU 測試需要 wgpu adapter，沒有 adapter 時直接失敗，不跳過。

## 寫計畫時已經完成的驗證

以下都在 scratch crate 裡用上面的確切版本編譯並執行過：

- **lyon 1.0.19**：`Path::builder()` 的 `begin`／`line_to`／`cubic_bezier_to`／`end(close)`；`StrokeTessellator` 配 `StrokeOptions::tolerance(t).with_line_width(w).with_line_join(LineJoin::Round).with_line_cap(LineCap::Round)`；`FillTessellator` 配 `FillOptions::tolerance(t).with_fill_rule(FillRule::EvenOdd)`；`BuffersBuilder::new(&mut buffers, |v: StrokeVertex| ...)`。`path.iter().flattened(tolerance)` 需要 `use lyon::path::iterator::PathIterator;`，只產生 `Begin`／`Line`／`End`。
- **wgpu 30.0.1**：`DepthStencilState` 的 `depth_write_enabled` 與 `depth_compare` 是 `Option`；stencil 用 `StencilFaceState { compare, fail_op, depth_fail_op, pass_op }`。無視窗的 device 用 `pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))` 和 `adapter.request_device(&DeviceDescriptor::default())`；`device.on_uncaptured_error(Arc::new(|e: wgpu::Error| ...))`；讀回用 `copy_texture_to_buffer` → `map_async` → `device.poll(wgpu::PollType::wait_indefinitely())` → `get_mapped_range()`（回傳 `Result`）。Task 6 的測試輔助程式（下方完整列出）在這台機器的 Intel Arc B390 上讀回正確的清除色。
- **glyphon 0.12.0／cosmic-text 0.19.0**：`TextRenderer::new(&mut atlas, &device, MultisampleState { count: 4, .. }, Some(depth_stencil_state))`；多個 `TextRenderer` 共用一個 `TextAtlas`；`TextArea` 有 `buffer`、`left`、`top`、`scale`、`bounds`、`default_color`、`custom_glyphs`。`Buffer::set_wrap(Wrap::None)`、`set_size(Some(w), None)`、`set_text(text, &attrs, Shaping::Advanced, None)` 都不帶 `FontSystem`，排版在 `shape_until_scroll(&mut font_system, false)` 才發生；`FontSystem::new()` 後用 `db_mut().load_font_data(bytes)` 載入打包字型。
- **文字基線**：cosmic-text 的 `line_y = line_top + (line_height - (ascent + descent)) / 2 + ascent`。napkin-hand、字級 20、行高 25 的純拉丁字母行，`line_y` 是 17.62，和 Excalidraw `getVerticalOffset` 的結果完全相同。同一行含中文（落到系統 Noto Sans CJK）時 `line_y` 變成 20.36，因為該行的最大 ascent 變大；所以 Task 7 每一行用一個 `Buffer`，自己對齊基線。
- **合併字型的垂直度量**（fontTools 讀出）：napkin-hand upem 1000、hhea 886／-374；napkin-sans 1011／-353；napkin-code 750／-250（`USE_TYPO_METRICS` 關閉，OS/2 win 是 1167／564）。前兩個和 Excalidraw `FONT_METADATA` 相同；napkin-code 要由 Task 7 的測試確認 cosmic-text 取的是 hhea。
- **eframe 0.36.2**：`App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`、`App::on_exit(&mut self)`；`NativeOptions::stencil_buffer: 8` 產生 `Stencil8`，每幀清成 0，sample count 跟 `multisampling` 一致。egui-wgpu 優先選 `Rgba8Unorm`／`Bgra8Unorm`，混色在 gamma 空間，和瀏覽器 canvas 相同，所以顏色直接用 sRGB 數值。
- **Wayland 讀取迴圈**：`EventQueue::prepare_read()` → `ReadEventsGuard::connection_fd()` → `rustix::event::poll` 同時等連線與 `rustix::event::eventfd` → `guard.read()` → `dispatch_pending`，可編譯。

## 與 spec 不同的地方

1. **半透明與箭頭標籤挖洞改用 stencil，不用離屏 texture（spec §6.2）。** 離屏做法要替每個可見的半透明元件準備一張視窗大小的 texture 才能照順序合成，幾十個元件就是數百 MB。stencil 做法：這類元件依序拿一個 1 到 255 的參考值，pipeline 只在 stencil 不等於參考值時寫入並把 stencil 設成參考值，元件內部由最上層的繪製部分往下畫。對不透明顏色，結果和 Excalidraw「元件先畫到自己的 canvas 再以透明度合成」相同。差異只在元件自己的顏色本身帶 alpha（例如 `#ff000080`）時：Excalidraw 在元件 canvas 內先混色，napkin 由最上層的部分獨佔像素。帶標籤的箭頭先用只寫 stencil 的四邊形蓋住標籤外框加 5（`BOUND_TEXT_PADDING`）的範圍，再畫箭頭。
2. **文字大小跟著連續的縮放倍率走（spec §6.3 寫跨級距才重建）。** glyphon 的 `TextArea::scale` 直接用 `zoom × pixels_per_point`，排版在場景單位只做一次；字形快取由 glyphon 的 atlas 管，每幀 `trim()`。圖形的三角化仍然照 2 的次方級距。
3. **旋轉過的文字用離屏 texture。** glyphon 不能旋轉；spec 沒有處理。這類文字元件很少見，每個依 `(id, version, scale)` 快取一張剛好包住文字的 texture。
4. **開檔用命令列參數。** spec §5.4 的「開上次的畫布」與自動建立 `scratch` 會寫檔，留給有存檔的里程碑。沒有參數時顯示空白畫布。
5. **frame 不裁切子元件。** Excalidraw 在 `frameRendering.clip` 開啟時把 frame 內的元件裁在 frame 範圍內；spec 沒提，M3 不做。frame 本身照 spec §1.2 畫虛線框，但它的 `opacity` 會乘到子元件上（Excalidraw `getRenderOpacity`）。
6. **捏合縮放照 M0 結論**：在 eframe 的 Wayland 連線上自己綁 `zwp_pointer_gestures_v1`，不是 spec §7.1 寫的 winit 事件。

## 寫計畫時做的決定

1. **字型沿用 `assets/fonts/` 的合併 TTF**（與使用者確認）。M0 記錄的字距落差只影響同時出現在 Latin-Ext 與越南文子集的字母；M3 的文字位置與寬度讀檔案存的值，落差到 M4 量文字寬度時才可能改變換行。
2. **帶標籤的箭頭在 M3 就挖洞**（與使用者確認）。標籤位置用檔案存的座標；依綁定重新定位是 M5。
3. **捏合縮放是 M3 最後一個任務**（與使用者確認）；非 Wayland 或 compositor 沒提供協定時只有 `Ctrl`+滾輪。
4. **相機語意和 Excalidraw 相同**：場景點 `p` 出現在畫布左上角起算 `(p + scroll) × zoom` 個邏輯點的位置；`zoom` 夾在 0.1 到 30。滾輪平移、`Shift`+滾輪水平平移、`Ctrl`+滾輪縮放的公式照 `packages/excalidraw/components/App.tsx` 的 `handleWheel`（第 13991 行起）。`appState.napkin` 存的視角用同一組語意（spec §5.4）。
5. **初始視角**：`appState.napkin` 有效就用它；否則 zoom 1，把所有未刪除元件的外框中心放在畫布中央；沒有元件時 scroll 為 0。
6. **三角化容許誤差**：畫面上 0.25 實體像素。級距 `bucket = ceil(log2(zoom × pixels_per_point))`，夾在 -8 到 8；場景單位的容許誤差是 `0.25 / 2^bucket`。
7. **快取**：形狀依 `(id, version, dark)`；網格依 `(id, version, dark, alpha, bucket)`，`alpha` 是元件透明度乘上所屬 frame 的透明度，直接烘進頂點顏色。版本號沿用檔案的 `version`；M3 不改元件，載入新檔時整個清空。
8. **剔除分兩層**：先用元件的放置外框（旋轉後的 AABB，加上 `64 + 8 × strokeWidth` 的餘裕）挑出可能可見的元件，只替它們建網格；再用網格的精確外框剔除。畫面外很遠的元件在級距變化時不重新三角化。
9. **舊版檔案**：缺欄位而載入成 `Raw` 的元件照 spec §1.2 畫虛線框加類型名稱，不做 Excalidraw 的 `restore.ts` 遷移。
10. **旋轉中心**：一般元件是 `(width/2, height/2)`；line、arrow、freedraw 是 points 外框的中心。Excalidraw 對曲線用曲線本身的外框，旋轉過的曲線因此可能有些微位移。
11. **效能量測的定義**：spec §6.7 的「影格時間 p99 ≤ 8.3ms」在 120Hz 螢幕上等於每一幀都趕上 vsync。面板與 `--bench` 報兩個數字：相鄰兩幀間隔的 p99（判準：小於 12.5ms，也就是 1.5 個刷新週期，代表沒有掉幀）和 napkin 自己 CPU 工作時間的 p99（參考用）。
12. **GPU 測試照常執行**，需要 adapter，沒有就失敗。這台開發機有 GPU，repo 沒有 CI。
13. **影格時間面板用 `F12` 切換**，預設隱藏。

## 檔案結構

```
crates/app/
  Cargo.toml                 package `app`，lib `app` + bin `napkin`
  src/main.rs                命令列、開檔、eframe 啟動
  src/lib.rs                 模組宣告
  src/cli.rs                 命令列參數
  src/document.rs            讀檔與錯誤訊息
  src/theme.rs               omarchy 主題 → egui 配色
  src/viewer.rs              eframe::App：畫布區域、名稱、錯誤、面板、bench
  src/camera.rs              相機與場景矩形
  src/input.rs               egui 輸入 → 相機變化
  src/sample.rs              完整欄位的元件 JSON，給測試與效能測試檔
  src/fixture.rs             固定種子的效能測試場景
  src/stats.rs               影格時間統計
  src/bench.rs               --bench 的相機腳本
  src/pinch.rs               Wayland 捏合手勢（Task 9）
  src/render/mod.rs
  src/render/color.rs        CSS 顏色 → RGBA，深色模式
  src/render/path.rs         rough ops／freedraw 外框 → lyon Path，虛線切段
  src/render/tessellate.rs   元件形狀 → 網格
  src/render/buffers.rs      共用 GPU buffer 的區段配置
  src/render/cache.rs        形狀與網格快取
  src/render/plan.rs         每幀的繪製清單：順序、剔除、stencil、標籤洞
  src/render/text.rs         字型、度量、行排版、文字快取
  src/render/gpu.rs          CanvasRenderer：pipeline、上傳、prepare／paint
  src/render/callback.rs     egui_wgpu::CallbackTrait 包裝
  tests/support/mod.rs       無視窗渲染與讀回
  tests/gpu_shapes.rs
  tests/gpu_text.rs
  examples/perf_fixture.rs
crates/scene/src/element.rs  Task 1 新增的存取函數
```

---

### Task 1：`scene` 的元件放置與屬性存取

**Files:**
- Modify: `crates/scene/src/element.rs`
- Modify: `crates/scene/src/lib.rs`（re-export `Placement`）

**Interfaces:**
- Consumes: M2 的 `Element`、`ElementBase`、`Element::from_value`。
- Produces（後面所有任務都用）：

```rust
/// Where an element sits: its top-left origin, size and rotation in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
}

impl Element {
    /// `None` for a `Raw` element whose `x`, `y`, `width` or `height` is missing or not a
    /// number; a missing `angle` reads as 0.
    pub fn placement(&self) -> Option<Placement>;
    /// `frameId` when it is a string.
    pub fn frame_id(&self) -> Option<&str>;
    /// `opacity` (0 to 100); a `Raw` element without a numeric one reads as 100.
    pub fn opacity(&self) -> f64;
    /// The `type` string.
    pub fn kind(&self) -> &str;
    /// `version`; a `Raw` element without a numeric one reads as 0.
    pub fn version(&self) -> f64;
}
```

型別化元件的 `frameId` 在各結構的 `extra` 裡（`ElementBase` 沒有這個欄位）。寫一個私有的 `fn extra(&self) -> Option<&Map<String, Value>>` 共用。

- [ ] **Step 1：在 `element.rs` 的測試模組加測試並確認失敗**

沿用模組裡既有的 `rectangle()`：

```rust
#[test]
fn typed_elements_expose_placement_and_attributes() {
    let mut value = rectangle();
    value["angle"] = json!(0.5);
    value["opacity"] = json!(60);
    value["frameId"] = json!("frame-1");
    let element = Element::from_value(value);
    assert!(matches!(element, Element::Rectangle(_)));
    assert_eq!(
        element.placement(),
        Some(Placement { x: 1.0, y: 2.0, width: 3.0, height: 4.0, angle: 0.5 })
    );
    assert_eq!(element.frame_id(), Some("frame-1"));
    assert_eq!(element.opacity(), 60.0);
    assert_eq!(element.kind(), "rectangle");
    assert_eq!(element.version(), 3.0);
}

#[test]
fn raw_elements_expose_placement_and_attributes() {
    let image = Element::from_value(json!({
        "id": "i", "type": "image", "x": 5, "y": 6, "width": 7, "height": 8,
        "opacity": 40, "frameId": "f", "version": 9
    }));
    assert!(matches!(image, Element::Raw(_)));
    assert_eq!(
        image.placement(),
        Some(Placement { x: 5.0, y: 6.0, width: 7.0, height: 8.0, angle: 0.0 })
    );
    assert_eq!(image.frame_id(), Some("f"));
    assert_eq!(image.opacity(), 40.0);
    assert_eq!(image.kind(), "image");
    assert_eq!(image.version(), 9.0);

    let bare = Element::from_value(json!({ "id": "b", "type": "magic", "x": "no" }));
    assert_eq!(bare.placement(), None);
    assert_eq!(bare.frame_id(), None);
    assert_eq!(bare.opacity(), 100.0);
    assert_eq!(bare.version(), 0.0);
}
```

Run: `cargo test -p scene --lib element`
Expected: 編譯失敗，`placement` 等方法不存在。

- [ ] **Step 2：實作 `Placement` 與五個方法，在 `lib.rs` 加 `pub use crate::element::Placement;`**

- [ ] **Step 3：在 `crates/scene/tests/corpus.rs` 加一個涵蓋語料的測試**

```rust
#[test]
fn every_corpus_element_has_a_placement() {
    for (name, text) in corpus() {
        let file = SceneFile::from_json_str(&text).expect("corpus loads");
        for element in &file.elements {
            assert!(
                element.placement().is_some(),
                "{name}: {} {:?} has no placement",
                element.kind(),
                element.id()
            );
        }
    }
}
```

- [ ] **Step 4：驗證**

Run: `cargo test -p scene` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 5：Commit**

```bash
git add crates/scene
git commit -m "Add placement, frame, opacity, kind and version accessors to Element"
```

---

### Task 2：`app` crate 骨架、命令列、讀檔與主題

**Files:**
- Modify: `Cargo.toml`（workspace members 加 `crates/app`；`[workspace.dependencies]` 加 `scene`、`eframe`、`egui-wgpu`、`wgpu`、`glyphon`、`lyon`、`bytemuck`、`toml`、`pollster`，版本照 Global Constraints）
- Create: `crates/app/Cargo.toml`、`src/lib.rs`、`src/main.rs`、`src/cli.rs`、`src/document.rs`、`src/theme.rs`、`src/viewer.rs`

**Interfaces:**
- Consumes: `scene::SceneFile`、`scene::file::LoadError`（`Display`）、`scene::color::tinycolor`。
- Produces:

```rust
// cli.rs
pub const USAGE: &str = "usage: napkin [FILE.excalidraw] [--bench]";
#[derive(Debug, PartialEq)]
pub struct Cli { pub file: Option<std::path::PathBuf>, pub bench: bool }
/// Arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, String>;

// document.rs
pub struct Document { pub name: String, pub file: std::sync::Arc<scene::SceneFile> }
impl Document {
    /// An empty scene named "untitled".
    pub fn empty() -> Document;
    /// Reads and parses `path`; the name is the file stem. The error names the path.
    pub fn load(path: &std::path::Path) -> Result<Document, String>;
}

// theme.rs
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: egui::Color32,
    pub foreground: egui::Color32,
    pub accent: egui::Color32,
    pub selection: egui::Color32,
    pub muted: egui::Color32,
}
impl Theme {
    /// The dark palette used when the omarchy theme file is missing or invalid (spec §7.6).
    pub fn builtin() -> Theme;
    /// `mode` ("dark" or "light") plus the five colors as "#rrggbb" strings.
    pub fn parse(text: &str) -> Result<Theme, String>;
    pub fn load(path: &std::path::Path) -> Result<Theme, String>;
    pub fn visuals(&self) -> egui::Visuals;
}
/// `$HOME/.local/state/omarchy/current/theme/colors.toml`.
pub fn omarchy_theme_path() -> Option<std::path::PathBuf>;

// viewer.rs
pub struct Viewer { /* document, load_error, theme, bench flag, … */ }
impl Viewer {
    pub fn new(cc: &eframe::CreationContext<'_>, document: Document, load_error: Option<String>, bench: bool) -> Viewer;
}
impl eframe::App for Viewer { fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame); }
```

`crates/app/Cargo.toml`：

```toml
[package]
name = "app"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "napkin"
path = "src/main.rs"

[dependencies]
bytemuck.workspace = true
eframe.workspace = true
egui-wgpu.workspace = true
glyphon.workspace = true
lyon.workspace = true
rough.workspace = true
scene.workspace = true
serde_json.workspace = true
toml.workspace = true
wgpu.workspace = true

[dev-dependencies]
pollster.workspace = true
```

`glyphon`、`lyon`、`bytemuck`、`wgpu` 在 Task 2 還用不到，但先列進來讓後面的任務不必改 manifest；clippy 不會因為未使用的依賴報錯。

- [ ] **Step 1：`cli.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_file_and_bench_flag_in_any_order() {
        assert_eq!(parse(args(&[])), Ok(Cli { file: None, bench: false }));
        assert_eq!(
            parse(args(&["a.excalidraw"])),
            Ok(Cli { file: Some("a.excalidraw".into()), bench: false })
        );
        assert_eq!(
            parse(args(&["--bench", "a.excalidraw"])),
            Ok(Cli { file: Some("a.excalidraw".into()), bench: true })
        );
    }

    #[test]
    fn rejects_unknown_flags_and_extra_files() {
        assert!(parse(args(&["--nope"])).is_err());
        assert!(parse(args(&["a.excalidraw", "b.excalidraw"])).is_err());
    }
}
```

- [ ] **Step 2：`document.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn corpus(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scene/tests/corpus").join(name)
    }

    #[test]
    fn loads_a_corpus_file_named_after_its_stem() {
        let document = Document::load(&corpus("shapes.excalidraw")).expect("loads");
        assert_eq!(document.name, "shapes");
        assert!(!document.file.elements.is_empty());
    }

    #[test]
    fn errors_name_the_path() {
        let missing = corpus("missing.excalidraw");
        let error = Document::load(&missing).err().expect("missing file fails");
        assert!(error.contains("missing.excalidraw"), "{error}");

        let invalid = std::env::temp_dir().join(format!("napkin-invalid-{}.excalidraw", std::process::id()));
        std::fs::write(&invalid, "{ not json").expect("write temp file");
        let error = Document::load(&invalid).err().expect("invalid JSON fails");
        std::fs::remove_file(&invalid).ok();
        assert!(error.contains("invalid JSON"), "{error}");
    }
}
```

- [ ] **Step 3：`theme.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const OZARK: &str = r##"
mode = "dark"
accent    = "#ef9268"
selection = "#3a3560"
muted     = "#9990b0"
background = "#171525"
foreground = "#e2dee8"
red = "#e8828a"
"##;

    #[test]
    fn parses_an_omarchy_theme_ignoring_extra_keys() {
        let theme = Theme::parse(OZARK).expect("parses");
        assert!(theme.dark);
        assert_eq!(theme.background, egui::Color32::from_rgb(0x17, 0x15, 0x25));
        assert_eq!(theme.accent, egui::Color32::from_rgb(0xef, 0x92, 0x68));
    }

    #[test]
    fn light_mode_and_errors() {
        let light = OZARK.replace("\"dark\"", "\"light\"");
        assert!(!Theme::parse(&light).expect("parses").dark);
        let missing = OZARK.replace("accent", "accentx");
        assert!(Theme::parse(&missing).unwrap_err().contains("accent"));
        let bad = OZARK.replace("#ef9268", "orange-ish");
        assert!(Theme::parse(&bad).is_err());
        assert!(Theme::parse("mode = \"sepia\"").is_err());
    }
}
```

- [ ] **Step 4：確認三組測試失敗**

Run: `cargo test -p app --lib`
Expected: 編譯失敗（函數不存在）。

- [ ] **Step 5：實作**

- `cli::parse`：`--bench` 設旗標；其他 `-` 開頭的參數回錯；第二個檔名回錯。
- `Document::load`：`std::fs::read_to_string` 失敗時回 `"{path}: {io error}"`；解析失敗回 `"{path}: {LoadError}"`（`LoadError::Json` 的 `Display` 以 `invalid JSON:` 開頭）。
- `Theme::parse`：用 `toml::Table`；顏色只接受 `#rrggbb`；錯誤訊息包含出錯的 key。`builtin()` 用 background `#1a1b26`、foreground `#c0caf5`、accent `#7aa2f7`、selection `#33467c`、muted `#565f89`，dark。`visuals()` 從 `egui::Visuals::dark()`／`light()` 出發，改 `panel_fill`、`window_fill`、`extreme_bg_color` 為 background，`override_text_color` 為 foreground，`selection.bg_fill` 為 selection，`hyperlink_color` 為 accent，`widgets.noninteractive.fg_stroke.color` 為 muted。
- `main.rs`：

```rust
use app::cli;
use app::document::Document;
use eframe::egui;

fn main() -> eframe::Result {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("napkin: {error}\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    let (document, load_error) = match &cli.file {
        None => (Document::empty(), None),
        Some(path) => match Document::load(path) {
            Ok(document) => (document, None),
            Err(error) => (Document::empty(), Some(error)),
        },
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin")
            .with_title(format!("{} - napkin", document.name)),
        renderer: eframe::Renderer::Wgpu,
        multisampling: 4,
        stencil_buffer: 8,
        ..Default::default()
    };
    let bench = cli.bench;
    eframe::run_native(
        "napkin",
        options,
        Box::new(move |cc| Ok(Box::new(app::viewer::Viewer::new(cc, document, load_error, bench)))),
    )
}
```

- `Viewer`：建立時讀主題（失敗時用 `Theme::builtin()` 並 `eprintln!` 原因）；`ui()` 每幀套用 `visuals()`；`ui.input(|i| i.focused)` 從 false 變 true 時重讀主題（spec §7.6）；中央面板填 theme background；右上角顯示 `document.name`；`load_error` 存在時在畫布中央用紅字顯示（spec §8：不覆寫，只顯示）。

- [ ] **Step 6：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

手動：`cargo run -p app -- crates/scene/tests/corpus/shapes.excalidraw` 開出標題為 `shapes - napkin` 的視窗，右上角顯示 `shapes`；`cargo run -p app -- /nonexistent.excalidraw` 顯示紅字錯誤；`hyprctl clients | grep -A3 napkin` 看到 `class: napkin`。

- [ ] **Step 7：Commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "Add the napkin app crate with CLI, file loading and omarchy theme"
```

---

### Task 3：相機與平移縮放輸入

**Files:**
- Create: `crates/app/src/camera.rs`、`crates/app/src/input.rs`
- Modify: `crates/app/src/lib.rs`、`crates/app/src/viewer.rs`

**Interfaces:**
- Consumes: Task 1 的 `Element::placement`、`Element::is_deleted`；Task 2 的 `Viewer`。
- Produces:

```rust
// camera.rs
pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneRect { pub min: [f64; 2], pub max: [f64; 2] }
impl SceneRect {
    pub fn intersects(&self, other: &SceneRect) -> bool;
    pub fn union(&self, other: &SceneRect) -> SceneRect;
    pub fn expand(&self, margin: f64) -> SceneRect;
    pub fn center(&self) -> [f64; 2];
    /// The axis-aligned bounds of `placement`'s rectangle rotated about `center`
    /// (local coordinates relative to the placement origin).
    pub fn of_rotated(placement: &scene::Placement, local_min: [f64; 2], local_max: [f64; 2], center: [f64; 2]) -> SceneRect;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera { pub scroll_x: f64, pub scroll_y: f64, pub zoom: f64 }
impl Default for Camera { /* scroll 0, zoom 1 */ }
impl Camera {
    /// `(p + scroll) * zoom`, in logical points from the canvas origin.
    pub fn scene_to_view(&self, p: [f64; 2]) -> [f64; 2];
    pub fn view_to_scene(&self, p: [f64; 2]) -> [f64; 2];
    /// Moves the content by `delta` logical points.
    pub fn pan_view(&mut self, delta: [f64; 2]);
    /// Sets `normalized_zoom(zoom)`, keeping the scene point under `anchor` in place.
    pub fn zoom_at(&mut self, zoom: f64, anchor: [f64; 2]);
    pub fn visible_rect(&self, view_size: [f64; 2]) -> SceneRect;
    /// Zoom 1 with `rect`'s center in the middle of the view.
    pub fn centered_on(rect: SceneRect, view_size: [f64; 2]) -> Camera;
    /// `ceil(log2(zoom * pixels_per_point))`, clamped to -8..=8.
    pub fn bucket(&self, pixels_per_point: f64) -> i32;
}
/// Excalidraw's `getNormalizedZoom`: rounded to 6 decimals, clamped to MIN_ZOOM..=MAX_ZOOM.
pub fn normalized_zoom(zoom: f64) -> f64;
/// The ctrl/cmd branch of `handleWheel`: the next zoom for a wheel `delta_y` in CSS pixels,
/// before normalization.
pub fn wheel_zoom(zoom: f64, delta_y: f64) -> f64;

// input.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel { pub delta: [f64; 2], pub ctrl: bool, pub shift: bool }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasInput {
    pub view_size: [f64; 2],
    /// Pointer position in logical points from the canvas origin.
    pub pointer: Option<[f64; 2]>,
    /// Wheel deltas in CSS-pixel convention: positive `y` scrolls down.
    pub wheels: Vec<Wheel>,
    /// Pointer movement while panning with Space+primary or middle drag.
    pub pan_drag: [f64; 2],
    /// Multiplicative zoom from a pinch gesture.
    pub pinch: Option<f64>,
}
/// Applies `input` to `camera`; returns whether the camera changed.
pub fn apply(camera: &mut Camera, input: &CanvasInput) -> bool;
impl CanvasInput {
    /// Reads this frame's events for the canvas `response`.
    pub fn from_egui(ui: &egui::Ui, response: &egui::Response) -> CanvasInput;
}
```

- [ ] **Step 1：`camera.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_transform_matches_excalidraw() {
        let camera = Camera { scroll_x: 10.0, scroll_y: -5.0, zoom: 2.0 };
        assert_eq!(camera.scene_to_view([0.0, 0.0]), [20.0, -10.0]);
        assert_eq!(camera.view_to_scene([20.0, -10.0]), [0.0, 0.0]);
    }

    #[test]
    fn zoom_at_keeps_the_anchor_fixed() {
        let mut camera = Camera::default();
        camera.zoom_at(2.0, [100.0, 50.0]);
        assert_eq!(camera.zoom, 2.0);
        assert_eq!((camera.scroll_x, camera.scroll_y), (-50.0, -25.0));
        camera.zoom_at(100.0, [0.0, 0.0]);
        assert_eq!(camera.zoom, MAX_ZOOM);
    }

    #[test]
    fn wheel_zoom_follows_handle_wheel() {
        // deltaY 50 is capped at MAX_STEP 10: 1 - 10/100; log10(1) adds nothing.
        assert!((wheel_zoom(1.0, 50.0) - 0.9).abs() < 1e-12);
        // 2 + 5/100 + log10(2) * 1 * min(1, 5/20).
        assert!((wheel_zoom(2.0, -5.0) - 2.125_257_498_915_995).abs() < 1e-12);
        // Math.sign(0) is 0, so a zero delta changes nothing.
        assert_eq!(wheel_zoom(3.0, 0.0), 3.0);
    }

    #[test]
    fn centered_on_and_buckets() {
        let rect = SceneRect { min: [0.0, 0.0], max: [200.0, 100.0] };
        let camera = Camera::centered_on(rect, [800.0, 600.0]);
        assert_eq!(camera.scene_to_view([100.0, 50.0]), [400.0, 300.0]);
        assert_eq!(Camera::default().bucket(1.25), 1);
        assert_eq!(Camera { zoom: 0.1, ..Default::default() }.bucket(1.0), -3);
    }
}
```

- [ ] **Step 2：`input.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn at(pointer: [f64; 2]) -> CanvasInput {
        CanvasInput { view_size: [800.0, 600.0], pointer: Some(pointer), ..Default::default() }
    }

    #[test]
    fn wheel_pans_by_css_pixels_over_zoom() {
        let mut camera = Camera { zoom: 2.0, ..Default::default() };
        let mut input = at([0.0, 0.0]);
        input.wheels.push(Wheel { delta: [10.0, 20.0], ctrl: false, shift: false });
        assert!(apply(&mut camera, &input));
        assert_eq!((camera.scroll_x, camera.scroll_y), (-5.0, -10.0));
    }

    #[test]
    fn shift_wheel_pans_horizontally() {
        let mut camera = Camera { zoom: 2.0, ..Default::default() };
        let mut input = at([0.0, 0.0]);
        input.wheels.push(Wheel { delta: [0.0, 30.0], ctrl: false, shift: true });
        apply(&mut camera, &input);
        assert_eq!((camera.scroll_x, camera.scroll_y), (-15.0, 0.0));
    }

    #[test]
    fn ctrl_wheel_zooms_about_the_pointer() {
        let mut camera = Camera::default();
        let mut input = at([200.0, 100.0]);
        input.wheels.push(Wheel { delta: [0.0, 50.0], ctrl: true, shift: false });
        apply(&mut camera, &input);
        assert_eq!(camera.zoom, 0.9);
        let anchor = camera.view_to_scene([200.0, 100.0]);
        assert!((anchor[0] - 200.0).abs() < 1e-9 && (anchor[1] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn drag_and_pinch() {
        let mut camera = Camera { zoom: 2.0, ..Default::default() };
        let mut input = at([100.0, 50.0]);
        input.pan_drag = [16.0, -4.0];
        apply(&mut camera, &input);
        assert_eq!((camera.scroll_x, camera.scroll_y), (8.0, -2.0));

        let mut camera = Camera::default();
        let mut input = CanvasInput { view_size: [800.0, 600.0], pinch: Some(2.0), ..Default::default() };
        apply(&mut camera, &input);
        // Without a pointer the pinch zooms about the view center.
        assert_eq!((camera.zoom, camera.scroll_x, camera.scroll_y), (2.0, -200.0, -150.0));
        input.pinch = None;
        assert!(!apply(&mut camera, &input));
    }
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p app --lib camera input`
Expected: 編譯失敗。

- [ ] **Step 4：實作**

- `wheel_zoom` 照 `handleWheel`：`sign` 在 0 時是 0（不要用 `f64::signum`，它對 0 回 1）；`MAX_STEP = ZOOM_STEP * 100 = 10`；`new = zoom - delta / 100`，再加 `log10(max(1, zoom)) * -sign * min(1, |deltaY| / 20)`；最後 `max(new, MIN_ZOOM)`。
- `normalized_zoom` 用 `rough::js::math_round(zoom * 1e6) / 1e6` 再夾範圍。
- `apply`：每個 wheel 依序處理；`ctrl` 時 `zoom_at(normalized_zoom(wheel_zoom(zoom, delta.y)), anchor)`；`shift` 時 `scroll_x -= (if delta.y != 0 { delta.y } else { delta.x }) / zoom`；其他 `scroll -= delta / zoom`。`pan_drag` 非零時 `pan_view`；`pinch` 時 `zoom_at(zoom * factor, anchor)`。anchor 是 pointer，沒有 pointer 時是 view 中心。
- `from_egui`：從 `ui.input(|i| i.events)` 讀 `egui::Event::MouseWheel { unit, delta, modifiers, .. }`，換算成 CSS 像素慣例：`Point` 原值、`Line` 乘 `ui.ctx().options(|o| o.input_options.line_scroll_speed)`、`Page` 乘 view 高度，再取負號（egui 的 `delta.y` 為正代表往上捲）；`ctrl` 取 `modifiers.ctrl || modifiers.command`。`pan_drag` 在 `response.dragged_by(PointerButton::Middle)`，或 `Space` 按住時 `response.dragged_by(PointerButton::Primary)`，取 `response.drag_delta()`。
- `Viewer`：畫布是中央面板裡 `ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag())` 的區域；每幀 `apply`，相機改變時不用自己 `request_repaint`（輸入已觸發重繪）。初始相機依決定 5：`appState.napkin`（`SceneFile::napkin_view`）或 `Camera::centered_on`，外框用所有未刪除元件 `placement()` 的矩形聯集（旋轉忽略，只用來定初始位置）。Task 3 先在畫布左上角用 egui 文字顯示 `zoom` 與 `scroll`，Task 6 拿掉。

- [ ] **Step 5：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

手動：開 `shapes.excalidraw`，雙指滑動改變 scroll，`Ctrl`+滾輪改變 zoom，`Space`+拖曳與中鍵拖曳改變 scroll。

- [ ] **Step 6：Commit**

```bash
git add crates/app
git commit -m "Add Excalidraw-compatible camera with wheel, drag and pinch input"
```

---

### Task 4：顏色、路徑、虛線與三角化

**Files:**
- Create: `crates/app/src/sample.rs`、`crates/app/src/render/mod.rs`、`render/color.rs`、`render/path.rs`、`render/tessellate.rs`
- Modify: `crates/app/src/lib.rs`

**Interfaces:**
- Consumes: `scene::shape::{ElementShape, PathOp, ShapeContext, generate_element_shape}`、`scene::color::{tinycolor, apply_dark_mode_filter}`、`rough::{Drawable, Op, OpSetType, Shape}`、Task 1 的 `Placement`、Task 3 的 `SceneRect`。
- Produces:

```rust
// sample.rs: complete element JSON for tests and the performance fixture.
pub fn generic(kind: &str, id: &str, rect: [f64; 4]) -> serde_json::Value;
pub fn linear(kind: &str, id: &str, origin: [f64; 2], points: &[[f64; 2]]) -> serde_json::Value;
pub fn freedraw(id: &str, origin: [f64; 2], points: &[[f64; 2]]) -> serde_json::Value;
pub fn text(id: &str, rect: [f64; 4], text: &str, container: Option<&str>) -> serde_json::Value;
/// Shallow merge: every key of `overrides` replaces the same key of `value`.
pub fn with(value: serde_json::Value, overrides: serde_json::Value) -> serde_json::Value;
pub fn file(elements: Vec<serde_json::Value>) -> scene::SceneFile;

// render/color.rs
/// Straight-alpha RGBA with gamma-encoded channels, which is what browser canvases blend.
pub type Rgba = [f32; 4];
pub fn css_color(value: &str) -> Option<Rgba>;
/// `css_color` after the dark-mode filter when `dark`; canvas keeps its default black for
/// a string it cannot parse.
pub fn render_color(value: &str, dark: bool) -> Rgba;

// render/path.rs
pub fn ops_path(ops: &[rough::Op]) -> lyon::path::Path;
pub fn outline_path(ops: &[scene::shape::PathOp]) -> lyon::path::Path;
/// Splits one flattened subpath into dashes following HTML canvas `setLineDash`: an odd
/// pattern repeats twice, the phase starts at 0 for every subpath, and a closed subpath
/// includes its closing segment.
pub fn dash_polyline(points: &[[f32; 2]], closed: bool, pattern: &[f32]) -> Vec<Vec<[f32; 2]>>;
/// `path` flattened with `tolerance` and dashed subpath by subpath.
pub fn dashed(path: &lyon::path::Path, pattern: &[f32], tolerance: f32) -> lyon::path::Path;

// render/tessellate.rs
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex { pub position: [f32; 2], pub color: [f32; 4] }

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    /// Indices relative to the first vertex of this mesh.
    pub indices: Vec<u32>,
    /// One index range per drawn layer, bottom to top.
    pub parts: Vec<std::ops::Range<u32>>,
    pub bounds: Option<crate::camera::SceneRect>,
}

pub struct Style {
    pub dark: bool,
    /// Element opacity times frame opacity, 0 to 1.
    pub alpha: f32,
    /// Scene-unit tolerance for the zoom bucket.
    pub tolerance: f32,
}

pub const VIEW_TOLERANCE_PX: f32 = 0.25;
pub fn tolerance_for_bucket(bucket: i32) -> f32;
/// The local rotation center (decision 10).
pub fn local_center(element: &scene::Element) -> Option<[f64; 2]>;
pub fn tessellate(element: &scene::Element, shape: &scene::shape::ElementShape, style: &Style) -> Mesh;
```

`sample.rs` 的欄位集合：`generic` 用 `crates/scene/src/element.rs` 測試裡 `rectangle()` 的欄位（`id`、`type`、`x`、`y`、`width`、`height` 換成參數，`seed` 用 1）；`linear`、`freedraw`、`text` 照 `crates/scene/tests/corpus/shapes.excalidraw` 與 `bindings.excalidraw` 裡同型別元件的完整 key 集合，數值用 Excalidraw 預設（`strokeColor` `#1e1e1e`、`backgroundColor` `transparent`、`strokeWidth` 2、`roughness` 1、`fontFamily` 5、`fontSize` 20、`lineHeight` 1.25、`textAlign` `left`、`verticalAlign` `top`）。`width`、`height` 對 line／arrow／freedraw 取 points 外框。`sample.rs` 自己的測試要求四種都載入成型別化元件。

- [ ] **Step 1：`sample.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use scene::Element;

    use super::*;

    #[test]
    fn samples_load_as_typed_elements() {
        let values = [
            generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0]),
            generic("diamond", "d", [0.0, 0.0, 10.0, 10.0]),
            generic("ellipse", "e", [0.0, 0.0, 10.0, 10.0]),
            linear("line", "l", [0.0, 0.0], &[[0.0, 0.0], [10.0, 5.0]]),
            linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 5.0]]),
            freedraw("f", [0.0, 0.0], &[[0.0, 0.0], [3.0, 4.0], [6.0, 1.0]]),
            text("t", [0.0, 0.0, 40.0, 25.0], "hi", None),
        ];
        for value in values {
            let element = Element::from_value(value.clone());
            assert!(!matches!(element, Element::Raw(_)), "{value}");
        }
        let file = file(vec![generic("rectangle", "r", [0.0, 0.0, 1.0, 1.0])]);
        assert_eq!(file.elements.len(), 1);
    }
}
```

- [ ] **Step 2：`color.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use scene::color::apply_dark_mode_filter;

    use super::*;

    #[test]
    fn parses_css_colors() {
        assert_eq!(css_color("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(css_color("rgba(255, 0, 0, 0.5)"), Some([1.0, 0.0, 0.0, 0.5]));
        assert_eq!(css_color("transparent").map(|c| c[3]), Some(0.0));
        assert_eq!(css_color("not a color"), None);
    }

    #[test]
    fn render_color_applies_dark_mode_and_defaults_to_black() {
        assert_eq!(render_color("not a color", false), [0.0, 0.0, 0.0, 1.0]);
        let expected = css_color(&apply_dark_mode_filter("#1e1e1e")).expect("filter output parses");
        assert_eq!(render_color("#1e1e1e", true), expected);
    }
}
```

- [ ] **Step 3：`path.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_a_straight_line() {
        let dashes = dash_polyline(&[[0.0, 0.0], [100.0, 0.0]], false, &[8.0, 10.0]);
        assert_eq!(dashes.len(), 6);
        assert_eq!(dashes[0], vec![[0.0, 0.0], [8.0, 0.0]]);
        assert_eq!(dashes[5], vec![[90.0, 0.0], [98.0, 0.0]]);
    }

    #[test]
    fn odd_patterns_repeat_twice() {
        let dashes = dash_polyline(&[[0.0, 0.0], [100.0, 0.0]], false, &[5.0]);
        assert_eq!(dashes.len(), 10);
        assert_eq!(dashes[1], vec![[10.0, 0.0], [15.0, 0.0]]);
    }

    #[test]
    fn dashes_turn_corners_and_include_the_closing_segment() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let dashes = dash_polyline(&square, true, &[15.0, 5.0]);
        assert_eq!(dashes.len(), 2);
        assert_eq!(dashes[0], vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0]]);
        assert_eq!(dashes[1], vec![[10.0, 10.0], [0.0, 10.0], [0.0, 5.0]]);
    }

    #[test]
    fn every_subpath_restarts_the_pattern() {
        use lyon::path::PathEvent;

        let mut builder = lyon::path::Path::builder();
        for y in [0.0, 10.0] {
            builder.begin(lyon::math::point(0.0, y));
            builder.line_to(lyon::math::point(20.0, y));
            builder.end(false);
        }
        let dashed = dashed(&builder.build(), &[8.0, 4.0], 0.1);
        let subpaths = dashed.iter().filter(|e| matches!(e, PathEvent::Begin { .. })).count();
        assert_eq!(subpaths, 4);
    }
}
```

- [ ] **Step 4：`tessellate.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use scene::Element;
    use scene::shape::{ShapeContext, generate_element_shape};
    use serde_json::json;

    use super::*;
    use crate::sample;

    fn mesh_of(value: serde_json::Value, alpha: f32) -> Mesh {
        let element = Element::from_value(value);
        let ctx = ShapeContext { dark_mode: false, canvas_background_color: "#ffffff" };
        let shape = generate_element_shape(&element, &ctx);
        tessellate(&element, &shape, &Style { dark: false, alpha, tolerance: 0.25 })
    }

    fn solid_rectangle(angle: f64) -> serde_json::Value {
        sample::with(
            sample::generic("rectangle", "r", [10.0, 20.0, 100.0, 50.0]),
            json!({ "roughness": 0, "backgroundColor": "#ffc9c9", "fillStyle": "solid", "angle": angle }),
        )
    }

    #[test]
    fn solid_rectangle_has_fill_then_stroke() {
        let mesh = mesh_of(solid_rectangle(0.0), 1.0);
        assert_eq!(mesh.parts.len(), 2);
        let first = mesh.vertices[mesh.indices[mesh.parts[0].start as usize] as usize];
        let fill = css_color_or_panic("#ffc9c9");
        assert_eq!(first.color, fill);
        let bounds = mesh.bounds.expect("non-empty mesh");
        assert!(bounds.min[0] <= 10.0 && bounds.min[0] >= 7.0, "{bounds:?}");
        assert!(bounds.max[1] >= 70.0 && bounds.max[1] <= 73.0, "{bounds:?}");
    }

    #[test]
    fn rotation_uses_the_element_center() {
        let mesh = mesh_of(solid_rectangle(std::f64::consts::FRAC_PI_2), 1.0);
        let bounds = mesh.bounds.expect("non-empty mesh");
        // Center (60, 45); a quarter turn swaps the 100x50 extents.
        assert!((bounds.min[0] - 35.0).abs() < 3.0 && (bounds.max[0] - 85.0).abs() < 3.0, "{bounds:?}");
        assert!((bounds.min[1] + 5.0).abs() < 3.0 && (bounds.max[1] - 95.0).abs() < 3.0, "{bounds:?}");
    }

    #[test]
    fn alpha_multiplies_vertex_colors() {
        let mesh = mesh_of(solid_rectangle(0.0), 0.5);
        assert!(mesh.vertices.iter().all(|v| (v.color[3] - 0.5).abs() < 1e-6));
    }

    #[test]
    fn raw_elements_become_dashed_placeholder_boxes() {
        let image = json!({ "id": "i", "type": "image", "x": 0, "y": 0, "width": 40, "height": 30, "angle": 0 });
        let mesh = mesh_of(image, 1.0);
        assert!(!mesh.indices.is_empty());
        let bounds = mesh.bounds.expect("non-empty mesh");
        assert!(bounds.max[0] <= 41.0 && bounds.max[1] <= 31.0, "{bounds:?}");
    }

    #[test]
    fn freedraw_outline_uses_the_stroke_color() {
        let value = sample::with(
            sample::freedraw("f", [0.0, 0.0], &[[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]]),
            json!({ "strokeColor": "#e03131" }),
        );
        let mesh = mesh_of(value, 1.0);
        let last = mesh.parts.last().expect("outline part");
        let color = mesh.vertices[mesh.indices[last.start as usize] as usize].color;
        assert_eq!(color, css_color_or_panic("#e03131"));
    }

    #[test]
    fn even_odd_fill_leaves_the_hole_uncovered() {
        let mut builder = lyon::path::Path::builder();
        for (lo, hi) in [(0.0, 100.0), (25.0, 75.0)] {
            builder.begin(lyon::math::point(lo, lo));
            builder.line_to(lyon::math::point(hi, lo));
            builder.line_to(lyon::math::point(hi, hi));
            builder.line_to(lyon::math::point(lo, hi));
            builder.end(true);
        }
        let path = builder.build();
        assert!(covers(&fill_for_test(&path, lyon::tessellation::FillRule::NonZero), [50.0, 50.0]));
        assert!(!covers(&fill_for_test(&path, lyon::tessellation::FillRule::EvenOdd), [50.0, 50.0]));
    }

    fn css_color_or_panic(value: &str) -> [f32; 4] {
        crate::render::color::css_color(value).expect("valid color")
    }

    /// Whether any triangle contains `p`.
    fn covers(buffers: &lyon::tessellation::VertexBuffers<[f32; 2], u32>, p: [f32; 2]) -> bool {
        buffers.indices.chunks(3).any(|t| {
            let [a, b, c] = [0, 1, 2].map(|i| buffers.vertices[t[i] as usize]);
            let side = |u: [f32; 2], v: [f32; 2]| (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]);
            let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
            !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
        })
    }

    fn fill_for_test(path: &lyon::path::Path, rule: lyon::tessellation::FillRule) -> lyon::tessellation::VertexBuffers<[f32; 2], u32> {
        use lyon::tessellation::{BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers};
        let mut buffers = VertexBuffers::new();
        FillTessellator::new()
            .tessellate_path(path, &FillOptions::tolerance(0.1).with_fill_rule(rule), &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()))
            .expect("tessellates");
        buffers
    }
}
```

`even_odd_fill_leaves_the_hole_uncovered` 驗的是 lyon 本身的填充規則，確保 `tessellate` 依 shape 選規則時真的有差別；`tessellate` 內部請用同一個 `FillOptions::with_fill_rule`。

- [ ] **Step 5：確認失敗**

Run: `cargo test -p app --lib sample render`
Expected: 編譯失敗。

- [ ] **Step 6：實作**

- `css_color`：`tinycolor(value)`，`ok` 為 false 回 `None`；`r/g/b` 除以 255、夾在 0 到 1；`a` 原值。
- `ops_path`：`Move` 時若有開著的 subpath 先 `end(false)` 再 `begin`；`LineTo` → `line_to`；`BCurveTo` → `cubic_bezier_to`；結束時關掉最後一個 subpath（不閉合）。第一個 op 不是 `Move` 時從 `(0, 0)` 開始（canvas 在沒有 current point 時的行為）。
- `outline_path`：`Move` → `begin`；`Quad` → `quadratic_bezier_to`；`Line` → `line_to`；`Close` → `end(true)`；結尾若還開著就 `end(false)`。
- `dash_polyline`：沿折線累積長度，依 pattern 交替開關；段落跨越頂點時把頂點放進當前 dash；長度為 0 的 dash（例如 dotted 的 1.5 在很短的 segment 上）仍輸出起點與終點相同的兩點，讓圓頭端點畫出一個點。pattern 全為 0 或含負數時不切段，直接回傳整條折線（canvas 忽略無效的 dash）。
- `tessellate`：
  - 變換：`scene = [x, y] + center + R(angle)(local - center)`，`center` 由 `local_center` 算出，套在所有頂點上。
  - `ElementShape::Drawables`：每個 drawable、每個 op set 依序產生一個 part。`Path` 描邊：寬度 `stroke_width`，顏色 `stroke`，`stroke_line_dash` 非空時先 `dashed`，圓頭圓角。`FillPath` 填充：顏色 `fill`，規則在 `shape` 是 `Curve`、`Polygon`、`Path` 時用 `EvenOdd`，其他 `NonZero`（roughjs `canvas.js` 的 `_drawToContext`）。`FillSketch` 描邊：寬度 `fill_weight`，小於 0 時用 `stroke_width / 2`，顏色 `fill`，`fill_line_dash` 非空時先 `dashed`。顏色字串是 `"none"` 時跳過該 part；顏色字串已經在 scene 套過深色模式，這裡用 `render_color(value, false)`。
  - `ElementShape::Freedraw`：`fill` 照上面處理；`stroke` 用 `outline_path` 以 `NonZero` 填充，顏色是 `render_color(strokeColor, style.dark)`（Excalidraw 在繪製時才對 freedraw 線條色套深色模式）。
  - `ElementShape::Placeholder` 與 `Element::Raw`：`(0,0)`–`(width,height)` 的矩形，寬 1，虛線 `[6, 4]`，顏色 `render_color("#868e96", style.dark)`。沒有 placement 時回傳空網格。
  - `ElementShape::None`：空網格。
  - 每個頂點的 alpha 乘上 `style.alpha`；`bounds` 取所有頂點位置的外框，沒有頂點時 `None`。
- `tolerance_for_bucket(b) = VIEW_TOLERANCE_PX / 2^b`。

- [ ] **Step 7：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 8：Commit**

```bash
git add crates/app
git commit -m "Tessellate rough drawables and freedraw outlines with lyon"
```

---

### Task 5：快取、GPU buffer 區段與每幀繪製清單

**Files:**
- Create: `crates/app/src/render/buffers.rs`、`render/cache.rs`、`render/plan.rs`

**Interfaces:**
- Consumes: Task 3 `Camera`、`SceneRect`；Task 4 `tessellate`、`Style`、`Mesh`、`tolerance_for_bucket`、`local_center`；Task 1 accessors。
- Produces:

```rust
// buffers.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment { pub vertex_start: u32, pub index_start: u32, pub index_count: u32 }
pub struct SegmentAllocator { /* capacities and bump pointers */ }
impl SegmentAllocator {
    pub fn new(vertex_capacity: u32, index_capacity: u32) -> SegmentAllocator;
    /// `None` when either buffer is full.
    pub fn allocate(&mut self, vertices: u32, indices: u32) -> Option<Segment>;
    pub fn reset(&mut self, vertex_capacity: u32, index_capacity: u32);
    pub fn used(&self) -> (u32, u32);
}

// cache.rs
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct MeshKey { pub id: String, pub version_bits: u64, pub dark: bool, pub alpha_bits: u32, pub bucket: i32 }
pub struct CachedMesh { pub mesh: std::sync::Arc<Mesh>, pub segment: Option<Segment> }
pub struct SceneCache { /* shapes: (id, version_bits, dark) -> Arc<ElementShape>; meshes: MeshKey -> CachedMesh + last-used frame */ }
impl SceneCache {
    pub fn new() -> SceneCache;
    /// Drops everything; call when a different file is loaded.
    pub fn clear(&mut self);
    pub fn begin_frame(&mut self);
    pub fn mesh(&mut self, element: &scene::Element, key: &MeshKey, canvas_background: &str) -> &mut CachedMesh;
    /// Forgets GPU segments after the shared buffers were recreated.
    pub fn forget_segments(&mut self);
    /// Drops meshes not used during the last `frames` frames.
    pub fn evict(&mut self, frames: u64);
}

// plan.rs
pub const COARSE_MARGIN: f64 = 64.0;
pub const BOUND_TEXT_PADDING: f64 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct ElementDraw { pub element: usize, pub key: MeshKey }

#[derive(Clone, Debug, PartialEq)]
pub enum DrawItem {
    Meshes(Vec<ElementDraw>),
    /// Drawn top layer first with stencil reference `stencil`; `hole` is the scene-space
    /// quad masked out before drawing (an arrow label).
    Isolated { draw: ElementDraw, stencil: u8, hole: Option<[[f64; 2]; 4]> },
    /// Writes 0 to the whole stencil buffer before references wrap around.
    StencilReset,
    /// Consecutive unrotated texts; `label` marks a placeholder's type label.
    Text(Vec<TextDraw>),
    RotatedText(TextDraw),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextDraw { pub element: usize, pub label: bool }

pub struct View { pub visible: SceneRect, pub dark: bool, pub bucket: i32 }

/// The draw list for one frame, in file order.
pub fn plan_frame(file: &scene::SceneFile, cache: &mut SceneCache, view: &View) -> Vec<DrawItem>;
/// Union of the coarse bounds of all non-deleted elements.
pub fn content_bounds(file: &scene::SceneFile) -> Option<SceneRect>;
```

- [ ] **Step 1：`buffers.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_until_full_then_resets() {
        let mut allocator = SegmentAllocator::new(10, 20);
        assert_eq!(allocator.allocate(4, 6), Some(Segment { vertex_start: 0, index_start: 0, index_count: 6 }));
        assert_eq!(allocator.allocate(6, 14), Some(Segment { vertex_start: 4, index_start: 6, index_count: 14 }));
        assert_eq!(allocator.allocate(1, 0), None);
        allocator.reset(100, 100);
        assert_eq!(allocator.used(), (0, 0));
        assert!(allocator.allocate(1, 3).is_some());
    }
}
```

- [ ] **Step 2：`plan.rs` 的測試**

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample;

    fn view() -> View {
        View { visible: SceneRect { min: [-1000.0, -1000.0], max: [1000.0, 1000.0] }, dark: false, bucket: 0 }
    }

    fn kinds(items: &[DrawItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                DrawItem::Meshes(draws) => format!("meshes{}", draws.len()),
                DrawItem::Isolated { stencil, hole, .. } => format!("isolated{stencil}{}", if hole.is_some() { "+hole" } else { "" }),
                DrawItem::StencilReset => "reset".to_owned(),
                DrawItem::Text(texts) => format!("text{}", texts.len()),
                DrawItem::RotatedText(_) => "rotated".to_owned(),
            })
            .collect()
    }

    #[test]
    fn keeps_file_order_and_groups_neighbours() {
        let file = sample::file(vec![
            sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]),
            sample::generic("ellipse", "b", [20.0, 0.0, 10.0, 10.0]),
            sample::text("t", [0.0, 20.0, 40.0, 25.0], "hi", None),
            sample::with(sample::text("r", [0.0, 60.0, 40.0, 25.0], "turned", None), json!({ "angle": 1.0 })),
            sample::generic("diamond", "c", [40.0, 0.0, 10.0, 10.0]),
            sample::with(sample::generic("rectangle", "gone", [0.0, 0.0, 5.0, 5.0]), json!({ "isDeleted": true })),
        ]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes2", "text1", "rotated", "meshes1"]);
    }

    #[test]
    fn culls_elements_outside_the_view() {
        let file = sample::file(vec![
            sample::generic("rectangle", "near", [0.0, 0.0, 10.0, 10.0]),
            sample::generic("rectangle", "far", [5000.0, 5000.0, 10.0, 10.0]),
        ]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes1"]);
    }

    #[test]
    fn translucent_elements_are_isolated_and_references_wrap() {
        let translucent = |i: usize| {
            sample::with(sample::generic("rectangle", &format!("t{i}"), [0.0, 0.0, 10.0, 10.0]), json!({ "opacity": 50 }))
        };
        let file = sample::file((0..256).map(translucent).collect());
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        let names = kinds(&items);
        assert_eq!(names[0], "isolated1");
        assert_eq!(names[254], "isolated255");
        assert_eq!(names[255], "reset");
        assert_eq!(names[256], "isolated1");
        let DrawItem::Isolated { draw, .. } = &items[0] else { panic!("isolated") };
        assert_eq!(draw.key.alpha_bits, 0.5f32.to_bits());
    }

    #[test]
    fn frame_opacity_multiplies_and_frames_are_placeholders() {
        let frame = json!({ "id": "f", "type": "frame", "x": -5, "y": -5, "width": 100, "height": 100, "angle": 0, "opacity": 50, "version": 1 });
        let child = sample::with(sample::generic("rectangle", "c", [0.0, 0.0, 10.0, 10.0]), json!({ "opacity": 50, "frameId": "f" }));
        let file = sample::file(vec![frame, child]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes1", "text1", "isolated1"]);
        let DrawItem::Text(labels) = &items[1] else { panic!("label") };
        assert!(labels[0].label);
        let DrawItem::Isolated { draw, .. } = &items[2] else { panic!("isolated") };
        assert_eq!(draw.key.alpha_bits, 0.25f32.to_bits());
    }

    #[test]
    fn labelled_arrows_get_a_padded_hole() {
        let arrow = sample::with(
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [200.0, 0.0]]),
            json!({ "boundElements": [{ "id": "label", "type": "text" }] }),
        );
        let label = sample::text("label", [80.0, -12.0, 40.0, 25.0], "hi", Some("a"));
        let file = sample::file(vec![arrow, label]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["isolated1+hole", "text1"]);
        let DrawItem::Isolated { hole: Some(hole), .. } = &items[0] else { panic!("hole") };
        assert_eq!(hole[0], [75.0, -17.0]);
        assert_eq!(hole[2], [125.0, 18.0]);
    }
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p app --lib render::buffers render::plan`
Expected: 編譯失敗。

- [ ] **Step 4：實作**

- `SceneCache::mesh`：形狀快取沒有時用 `generate_element_shape(element, &ShapeContext { dark_mode: key.dark, canvas_background_color })` 產生；網格快取沒有時用 `tessellate(element, &shape, &Style { dark: key.dark, alpha: f32::from_bits(key.alpha_bits), tolerance: tolerance_for_bucket(key.bucket) })` 產生。`begin_frame` 遞增 frame 計數，`mesh` 更新該項目的最後使用 frame。
- `plan_frame`：
  1. 以 `id → index` 建表；找出每個 frame 的 `opacity`；找出每支 `Element::Arrow` 的標籤（`TextElement::container_id` 指向它、未刪除的文字）。
  2. 依檔案順序走過未刪除元件。粗略外框：`SceneRect::of_rotated(placement, [0,0] 或 points 外框, …)` 再 `expand(COARSE_MARGIN + 8 × strokeWidth)`，不和 `view.visible` 相交就跳過。
  3. 文字元件：`angle == 0` 加進 `Text`，否則 `RotatedText`。
  4. 其他元件：`alpha = opacity / 100 × frame opacity / 100`；取 `cache.mesh` 並用網格外框精確剔除（外框為 `None` 也跳過）。`alpha < 1` 或帶標籤時產生 `Isolated`，否則併入目前的 `Meshes`。標籤洞是標籤 placement 的矩形各邊外擴 `BOUND_TEXT_PADDING`，四個角依序為左上、右上、右下、左下（不考慮標籤旋轉，和 Excalidraw `clearRect` 相同）。
  5. `Raw` 元件與形狀為 `Placeholder` 的元件：虛線框併入 `Meshes`，接著加一個 `label: true` 的 `TextDraw`。虛線框與標籤是 napkin 自己的提示，不是元件內容，一律不透明（`alpha` 為 1），也不因此變成 `Isolated`。
  6. stencil 參考值從 1 開始遞增；要用第 256 個之前先插入 `StencilReset` 再從 1 開始。
- `content_bounds`：所有未刪除元件粗略外框（不加餘裕）的聯集。

- [ ] **Step 5：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

- [ ] **Step 6：Commit**

```bash
git add crates/app
git commit -m "Plan each canvas frame with mesh caching, culling and stencil isolation"
```

---

### Task 6：GPU 畫布與 paint callback

**Files:**
- Create: `crates/app/src/render/gpu.rs`、`render/callback.rs`、`crates/app/tests/support/mod.rs`、`crates/app/tests/gpu_shapes.rs`
- Modify: `crates/app/src/viewer.rs`（接上畫布，拿掉 Task 3 的相機文字）

**Interfaces:**
- Consumes: Task 5 全部；Task 3 `Camera`；Task 4 `Vertex`、`render_color`；Task 2 `Document`。
- Produces:

```rust
// gpu.rs
pub const SAMPLE_COUNT: u32 = 4;
pub const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

#[derive(Clone)]
pub struct CanvasFrame {
    pub file: std::sync::Arc<scene::SceneFile>,
    pub camera: crate::camera::Camera,
    /// Canvas size in physical pixels.
    pub size_px: [u32; 2],
    pub pixels_per_point: f32,
    pub dark: bool,
}

pub struct CanvasRenderer { /* pipelines, buffers, allocator, cache, last plan */ }
impl CanvasRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, target_format: wgpu::TextureFormat) -> CanvasRenderer;
    /// Clears caches; call when another file is shown.
    pub fn reset(&mut self);
    /// Plans the frame and uploads what it needs. The returned buffers (offscreen text in
    /// Task 7) must be submitted before the pass that calls `paint`.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &CanvasFrame) -> Vec<wgpu::CommandBuffer>;
    /// Draws the prepared frame into a pass with a 4x MSAA color target and a Stencil8
    /// attachment whose viewport is the canvas.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>);
    pub fn stats(&self) -> RenderStats;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderStats { pub drawn_elements: usize, pub cached_meshes: usize, pub buffer_vertices: u32 }

// callback.rs
pub struct CanvasCallback { pub frame: CanvasFrame }
impl egui_wgpu::CallbackTrait for CanvasCallback { /* prepare -> renderer.prepare, paint -> renderer.paint */ }
/// Inserts a `CanvasRenderer` into eframe's callback resources.
pub fn install(render_state: &egui_wgpu::RenderState);
```

**Pipelines**（同一個 WGSL shader，vertex 輸入 `position: vec2<f32>`、`color: vec4<f32>`；uniform 為 `scroll: vec2<f32>`、`zoom_px: f32`（zoom × pixels_per_point）、`viewport_px: vec2<f32>`；`clip = ((p + scroll) × zoom_px / viewport_px) × (2, -2) + (-1, 1)`；混色 `BlendState::ALPHA_BLENDING`）：

| pipeline | stencil compare | pass op | color writes | 用途 |
|---|---|---|---|---|
| `plain` | `Always` | `Keep` | ALL | `Meshes` |
| `isolated` | `NotEqual` | `Replace` | ALL | `Isolated` 的元件本身 |
| `mask` | `Always` | `Replace` | 無 | 標籤洞 |
| `reset` | `Always` | `Replace`（reference 0） | 無 | `StencilReset`，全畫布四邊形 |

每個 pipeline 的 `depth_stencil` 都是 `Some(DepthStencilState { format: Stencil8, depth_write_enabled: Some(false), depth_compare: Some(CompareFunction::Always), stencil: StencilState { front: face, back: face, read_mask: 0xff, write_mask: 0xff }, bias: Default::default() })`；`multisample.count` 為 4；`primitive.cull_mode` 為 `None`（lyon 的三角形方向不固定）。`reset` 的四邊形直接用 clip 座標 `(-1,-1)`–`(1,1)`，可以用另一個只輸出固定位置的 vertex entry point。

**上傳**：共用 vertex／index buffer（`VERTEX | COPY_DST`、`INDEX | COPY_DST`），初始容量 1M 頂點、3M 索引。`prepare` 為清單裡還沒有 `segment` 的網格配置區段並 `queue.write_buffer`；配置失敗時把容量加倍（至少能放下本幀全部可見網格），重建 buffer，`cache.forget_segments()`，本幀可見網格全部重新上傳。`paint` 對每個網格 part 呼叫 `draw_indexed(index_start + part.start .. index_start + part.end, vertex_start as i32, 0..1)`；`Meshes` 依 part 由下往上畫，`Isolated` 由上往下畫，畫前 `set_stencil_reference(stencil)`；有洞時先用 `mask` 畫洞的兩個三角形（每幀寫進一個小的動態 buffer）。`prepare` 結尾呼叫 `cache.evict(600)`。

**測試輔助程式** `crates/app/tests/support/mod.rs`（寫計畫時已在這台機器跑過）：

```rust
use app::camera::Camera;
use app::render::gpu::{CanvasFrame, CanvasRenderer, STENCIL_FORMAT};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Image { pub width: u32, pub height: u32, pub rgba: Vec<u8> }

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].try_into().expect("four channels")
    }
}

pub fn gpu() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU tests need a wgpu adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .expect("wgpu device");
    device.on_uncaptured_error(std::sync::Arc::new(|error: wgpu::Error| {
        panic!("wgpu validation error: {error}")
    }));
    (device, queue)
}

/// Renders `file` at pixels-per-point 1 over its own view background.
pub fn render(file: scene::SceneFile, camera: Camera, width: u32, height: u32, dark: bool) -> Image {
    let (device, queue) = gpu();
    let background = app::render::color::render_color(file.view_background_color(), dark);
    let mut renderer = CanvasRenderer::new(&device, &queue, FORMAT);
    let frame = CanvasFrame {
        file: std::sync::Arc::new(file),
        camera,
        size_px: [width, height],
        pixels_per_point: 1.0,
        dark,
    };
    let prepared = renderer.prepare(&device, &queue, &frame);
    queue.submit(prepared);

    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = |label, samples, format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label), size, mip_level_count: 1, sample_count: samples,
            dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[],
        })
    };
    let msaa = texture("msaa", 4, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let resolve = texture("resolve", 1, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let stencil = texture("stencil", 4, STENCIL_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let (msaa_view, resolve_view, stencil_view) = (
        msaa.create_view(&Default::default()),
        resolve.create_view(&Default::default()),
        stencil.create_view(&Default::default()),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let clear = wgpu::Color {
            r: f64::from(background[0]), g: f64::from(background[1]),
            b: f64::from(background[2]), a: f64::from(background[3]),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test canvas"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa_view, depth_slice: None, resolve_target: Some(&resolve_view),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &stencil_view, depth_ops: None,
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
            }),
            timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        });
        renderer.paint(&mut pass);
    }
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = (width * 4).div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"), size: u64::from(padded * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        resolve.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("map readback"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    let data = readback.slice(..).get_mapped_range().expect("mapped readback");
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        rgba.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    Image { width, height, rgba }
}
```

- [ ] **Step 1：`tests/gpu_shapes.rs`**

```rust
mod support;

use app::camera::Camera;
use app::sample;
use serde_json::json;

fn close(actual: [u8; 4], expected: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|i| actual[i].abs_diff(expected[i]) <= tolerance)
}

#[test]
fn solid_fill_and_background() {
    let rect = sample::with(
        sample::generic("rectangle", "r", [20.0, 20.0, 60.0, 40.0]),
        json!({ "roughness": 0, "backgroundColor": "#ffc9c9", "fillStyle": "solid" }),
    );
    let image = support::render(sample::file(vec![rect]), Camera::default(), 100, 80, false);
    assert!(close(image.pixel(50, 40), [0xff, 0xc9, 0xc9], 2), "{:?}", image.pixel(50, 40));
    assert!(close(image.pixel(5, 5), [0xff, 0xff, 0xff], 0), "{:?}", image.pixel(5, 5));
}

#[test]
fn translucent_double_stroke_never_darkens_twice() {
    // roughness 2 draws two offset strokes that cross; at opacity 50 a crossing pixel must
    // look like a single stroke over the background.
    let rect = sample::with(
        sample::generic("rectangle", "r", [20.0, 20.0, 160.0, 120.0]),
        json!({ "roughness": 2, "strokeWidth": 4, "opacity": 50 }),
    );
    let image = support::render(sample::file(vec![rect]), Camera::default(), 200, 160, false);
    // #1e1e1e at 50% over white is 0x8f; allow MSAA edge noise upwards only.
    let darkest = image.rgba.chunks(4).map(|p| p[0]).min().expect("pixels");
    assert!(darkest >= 0x8f - 2, "darkest channel {darkest:#x}");
    assert!(darkest <= 0x8f + 8, "stroke not drawn, darkest {darkest:#x}");
}

#[test]
fn arrow_label_hole_shows_the_background() {
    let arrow = sample::with(
        sample::linear("arrow", "a", [10.0, 50.0], &[[0.0, 0.0], [180.0, 0.0]]),
        json!({ "roughness": 0, "strokeWidth": 4, "boundElements": [{ "id": "label", "type": "text" }] }),
    );
    let label = sample::text("label", [80.0, 38.0, 40.0, 25.0], "hi", Some("a"));
    let image = support::render(sample::file(vec![arrow, label]), Camera::default(), 200, 100, false);
    // Inside the 5px padding left of the label box, on the arrow's line.
    assert!(close(image.pixel(77, 50), [0xff, 0xff, 0xff], 2), "{:?}", image.pixel(77, 50));
    // Away from the label the arrow is drawn.
    assert!(close(image.pixel(40, 50), [0x1e, 0x1e, 0x1e], 8), "{:?}", image.pixel(40, 50));
}

#[test]
fn camera_moves_content_and_culls() {
    let rect = sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 20.0, 20.0]),
        json!({ "roughness": 0, "backgroundColor": "#000000", "fillStyle": "solid" }),
    );
    let camera = Camera { scroll_x: 40.0, scroll_y: 30.0, zoom: 2.0 };
    let image = support::render(sample::file(vec![rect.clone()]), camera, 200, 200, false);
    // Scene (10, 10) lands at view ((10 + 40) * 2, (10 + 30) * 2).
    assert!(close(image.pixel(100, 80), [0, 0, 0], 2), "{:?}", image.pixel(100, 80));
    let away = Camera { scroll_x: -5000.0, ..Camera::default() };
    let empty = support::render(sample::file(vec![rect]), away, 50, 50, false);
    assert!(empty.rgba.chunks(4).all(|p| p[0] == 0xff));
}

#[test]
fn every_corpus_file_renders_without_validation_errors() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scene/tests/corpus");
    for entry in std::fs::read_dir(dir).expect("corpus dir") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("read corpus");
        let load = || scene::SceneFile::from_json_str(&text).expect("corpus loads");
        let bounds = app::render::plan::content_bounds(&load()).expect("content");
        let camera = Camera::centered_on(bounds, [400.0, 300.0]);
        for dark in [false, true] {
            // Rendering must finish without a wgpu validation error (the device panics).
            support::render(load(), camera, 400, 300, dark);
        }
    }
}
```

`sample::file` 的 `appState.viewBackgroundColor` 是 `#ffffff`。

- [ ] **Step 2：確認失敗**

Run: `cargo test -p app --test gpu_shapes`
Expected: 編譯失敗（`gpu` 模組不存在）。

- [ ] **Step 3：實作 `CanvasRenderer` 與 `callback.rs`**

參考 M0 `canvas_probe.rs` 的 pipeline 建立方式與 `CallbackTrait` 寫法。`Text` 與 `RotatedText` 項目在這個任務先略過，Task 7 才畫。`callback::install` 在 `Viewer::new` 用 `cc.wgpu_render_state` 呼叫；`CallbackTrait::prepare` 回傳 `renderer.prepare` 的結果。

- [ ] **Step 4：接上 `Viewer`**

畫布區域先用 `ui.painter().rect_filled(rect, 0.0, background)` 填上 `render_color(viewBackgroundColor, theme.dark)`，再加 `egui_wgpu::Callback::new_paint_callback(rect, CanvasCallback { frame })`，`size_px` 是 rect 大小乘 `pixels_per_point`。canvas 的深淺色跟 omarchy 主題的 `mode`。

- [ ] **Step 5：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

手動：`cargo run -p app -- crates/scene/tests/corpus/shapes.excalidraw`，和 `cargo run -p scene --example preview_svg -- crates/scene/tests/corpus/shapes.excalidraw /tmp/shapes.svg` 的輸出並排比對，線條、填色、虛線、手繪線要一致（文字與標籤在 Task 7）。

- [ ] **Step 6：Commit**

```bash
git add crates/app
git commit -m "Render scene meshes on the GPU through an egui paint callback"
```

---

### Task 7：文字

**Files:**
- Create: `crates/app/src/render/text.rs`、`crates/app/tests/gpu_text.rs`
- Modify: `crates/app/src/render/gpu.rs`

**Interfaces:**
- Consumes: Task 5 `DrawItem::{Text, RotatedText}`、`TextDraw`；Task 6 `CanvasRenderer`；`scene::element::TextElement`。
- Produces:

```rust
// render/text.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics { pub units_per_em: f64, pub ascender: f64, pub descender: f64 }

/// spec §6.3: 5, 1 -> napkin-hand; 6, 2, 7, 9, 10 -> napkin-sans; 8, 3 -> napkin-code;
/// anything else -> napkin-hand.
pub fn bundled_family(font_family: f64) -> &'static str;
/// `FONT_METADATA[fontFamily].metrics`, falling back to Excalifont's.
pub fn excalidraw_metrics(font_family: f64) -> FontMetrics;

#[derive(Clone, Debug, PartialEq)]
pub struct LineLayout { pub text: String, /* local baseline y */ pub baseline: f64 }
/// `renderElement.ts`'s text branch: lines split on \r\n, \r and \n; baseline of line i is
/// `i * fontSize * lineHeight + getVerticalOffset(...)`.
pub fn layout_lines(text: &scene::element::TextElement) -> Vec<LineLayout>;
/// Placeholder type label: napkin-sans 12, baseline 16, left 4, color #868e96.
pub fn placeholder_label(kind: &str) -> LineLayout;

/// A `FontSystem` with system fonts (CJK fallback) and the three bundled fonts.
pub fn font_system() -> glyphon::FontSystem;

pub struct ShapedLine {
    pub buffer: glyphon::Buffer,
    /// `line_y` of the buffer's first layout run, in scene units.
    pub baseline_in_buffer: f32,
    pub baseline: f64,
}
pub fn shape_lines(font_system: &mut glyphon::FontSystem, lines: &[LineLayout], family: &str, font_size: f32, line_height_px: f32, width: f32, align: Option<glyphon::cosmic_text::Align>) -> Vec<ShapedLine>;
```

`FONT_METADATA` 的值從 `packages/common/src/font-metadata.ts` 抄（包含 Virgil、Helvetica、Cascadia、Lilita One、Nunito、Excalifont、Comic Shanns 等所有有數字 id 的項目），每筆旁邊註明 family 名稱。

**畫進 CanvasRenderer：**
- 字型檔用 `include_bytes!("../../../../assets/fonts/napkin-hand.ttf")` 等載入。
- 文字快取以 `(id, version)` 為 key 存 `Vec<ShapedLine>`；`Buffer` 的 `Metrics::new(fontSize, fontSize × lineHeight)`，`Wrap::None`，寬度 `width`，對齊 `textAlign`（`center` → `Align::Center`、`right` → `Align::Right`、其他 `Align::Left`），每行一個 `Buffer`。
- `Text` 項目：每個項目用 `TextRenderer` 池中的一個（依項目順序取第 n 個，不夠就建立），`depth_stencil` 是 `plain` 的狀態、sample count 4、共用一個 `TextAtlas` 與 `Viewport`。每行的 `TextArea`：`scale = zoom × pixels_per_point`；`left` = 元件原點的畫面 x（實體像素）；`top` = `(元件 y + baseline + scroll_y) × zoom × ppp − baseline_in_buffer × scale`；`bounds` 為整個畫布；`default_color` 為 `render_color(strokeColor, dark)` 的 alpha 乘上元件與 frame 的透明度，換成 `glyphon::Color::rgba`。placeholder 標籤用 `placeholder_label`，位置相對於元件原點。所有 `TextRenderer::prepare` 做完後呼叫 `atlas.trim()`。
- `RotatedText`：以 `(id, version, scale.to_bits())` 快取一張 `Rgba8Unorm`（和畫布 target 格式相同）、sample count 1 的 texture，大小 `ceil(width × scale) + 2` × `ceil(height × scale) + 2`；用一個 sample count 1、`depth_stencil: None` 的 `TextRenderer` 在 `prepare` 裡另開 encoder 畫進去（清成透明），回傳 command buffer。`paint` 用 `textured` pipeline（sample count 4、stencil 同 `plain`、`BlendState::PREMULTIPLIED_ALPHA_BLENDING`，因為文字以 `ALPHA_BLENDING` 畫在透明底上的結果是預乘的）畫一個以元件中心旋轉 `angle` 的四邊形。

- [ ] **Step 1：`text.rs` 的單元測試**

```rust
#[cfg(test)]
mod tests {
    use scene::Element;
    use serde_json::json;

    use super::*;
    use crate::sample;

    fn text_element(overrides: serde_json::Value) -> scene::element::TextElement {
        match Element::from_value(sample::with(sample::text("t", [0.0, 0.0, 100.0, 50.0], "a", None), overrides)) {
            Element::Text(text) => text,
            other => panic!("not text: {other:?}"),
        }
    }

    #[test]
    fn families_and_metrics() {
        assert_eq!(bundled_family(5.0), "napkin-hand");
        assert_eq!(bundled_family(2.0), "napkin-sans");
        assert_eq!(bundled_family(3.0), "napkin-code");
        assert_eq!(bundled_family(42.0), "napkin-hand");
        assert_eq!(excalidraw_metrics(5.0), FontMetrics { units_per_em: 1000.0, ascender: 886.0, descender: -374.0 });
        assert_eq!(excalidraw_metrics(42.0), excalidraw_metrics(5.0));
    }

    #[test]
    fn lines_follow_get_vertical_offset() {
        let text = text_element(json!({ "text": "one\r\ntwo\rthree", "fontSize": 20, "lineHeight": 1.25 }));
        let lines = layout_lines(&text);
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["one", "two", "three"]);
        assert!((lines[0].baseline - 17.62).abs() < 1e-9);
        assert!((lines[2].baseline - (50.0 + 17.62)).abs() < 1e-9);
    }

    #[test]
    fn shaped_baselines_match_excalidraw_for_all_bundled_fonts() {
        let mut font_system = font_system();
        for font_family in [5.0, 6.0, 8.0] {
            let text = text_element(json!({ "text": "Hamburg", "fontFamily": font_family, "fontSize": 20, "lineHeight": 1.25 }));
            let lines = layout_lines(&text);
            let shaped = shape_lines(&mut font_system, &lines, bundled_family(font_family), 20.0, 25.0, 100.0, None);
            // Latin text uses the bundled font itself, so cosmic-text's baseline must equal
            // Excalidraw's vertical offset.
            assert!(
                (f64::from(shaped[0].baseline_in_buffer) - lines[0].baseline).abs() < 0.01,
                "family {font_family}: {} vs {}",
                shaped[0].baseline_in_buffer,
                lines[0].baseline
            );
        }
    }
}
```

`shaped_baselines_match_excalidraw_for_all_bundled_fonts` 若在 napkin-code（`fontFamily` 8）失敗，代表 cosmic-text 用了 OS/2 win 度量而不是 hhea；此時在 `shape_lines` 裡以 `excalidraw_metrics` 的 baseline 為準，對齊時改用 `lines[i].baseline` 與實際 `line_y` 的差（這正是每行一個 `Buffer` 的用意），並把這個測試改成斷言對齊後的畫面位置。

- [ ] **Step 2：`tests/gpu_text.rs`**

```rust
mod support;

use app::camera::Camera;
use app::sample;
use serde_json::json;

fn white_pixels(image: &support::Image, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y)[0] > 200)
        .count()
}

#[test]
fn text_interleaves_with_shapes() {
    let solid = |id: &str, rect: [f64; 4], color: &str| {
        sample::with(sample::generic("rectangle", id, rect), json!({
            "roughness": 0, "strokeColor": color, "backgroundColor": color, "fillStyle": "solid"
        }))
    };
    let under = solid("under", [10.0, 10.0, 180.0, 60.0], "#000000");
    let label = sample::with(sample::text("t", [20.0, 20.0, 160.0, 40.0], "MMMMMMMM", None), json!({
        "strokeColor": "#ffffff", "fontSize": 32, "fontFamily": 6
    }));
    let over = solid("over", [100.0, 5.0, 95.0, 70.0], "#1971c2");
    let image = support::render(sample::file(vec![under, label, over]), Camera::default(), 200, 80, false);
    assert!(white_pixels(&image, 20, 95, 20, 60) > 50, "text not drawn over the first rectangle");
    assert_eq!(white_pixels(&image, 105, 190, 10, 70), 0, "text drawn over the later rectangle");
}

#[test]
fn rotated_text_is_drawn_rotated() {
    let background = sample::with(sample::generic("rectangle", "bg", [0.0, 0.0, 200.0, 200.0]), json!({
        "roughness": 0, "strokeColor": "#000000", "backgroundColor": "#000000", "fillStyle": "solid"
    }));
    // A wide single-line text turned a quarter: its glyphs must end up in a tall band
    // around the center, not in the horizontal band it would occupy unrotated.
    let text = sample::with(sample::text("t", [20.0, 80.0, 160.0, 40.0], "MMMMMMMM", None), json!({
        "strokeColor": "#ffffff", "fontSize": 32, "fontFamily": 6, "angle": std::f64::consts::FRAC_PI_2
    }));
    let image = support::render(sample::file(vec![background, text]), Camera::default(), 200, 200, false);
    assert!(white_pixels(&image, 80, 120, 20, 180) > 50, "rotated glyphs missing");
    assert_eq!(white_pixels(&image, 20, 60, 80, 120), 0, "glyphs left in the unrotated position");
    assert_eq!(white_pixels(&image, 140, 180, 80, 120), 0, "glyphs left in the unrotated position");
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p app --lib render::text` 與 `cargo test -p app --test gpu_text`
Expected: 前者編譯失敗；後者兩個測試失敗（文字還沒畫）。

- [ ] **Step 4：實作 `text.rs` 並接進 `CanvasRenderer`**

- [ ] **Step 5：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過，包含 Task 6 的 `arrow_label_hole_shows_the_background`（取樣點在標籤外框與洞邊界之間，文字不會畫到）。

手動（spec §9.3 #3）：`cargo run -p app -- crates/scene/tests/corpus/bindings.excalidraw` 與 excalidraw.com 開同一檔案並排比對，中文字、容器文字、箭頭標籤位置與挖洞一致；把 omarchy 主題切成 light 與 dark 各看一次。

- [ ] **Step 6：Commit**

```bash
git add crates/app
git commit -m "Draw text with glyphon in layer order, including rotated text"
```

---

### Task 8：影格時間面板、效能測試場景與 `--bench`

**Files:**
- Create: `crates/app/src/stats.rs`、`crates/app/src/bench.rs`、`crates/app/src/fixture.rs`、`crates/app/examples/perf_fixture.rs`
- Modify: `crates/app/src/viewer.rs`、`crates/app/src/lib.rs`

**Interfaces:**
- Consumes: Task 4 `sample`；Task 6 `RenderStats`；`scene::fractional_index::generate_n_keys_between`。
- Produces:

```rust
// stats.rs
pub const WINDOW: usize = 1200;
/// Nearest-rank percentile; `None` for no samples.
pub fn percentile(samples: &[f64], p: f64) -> Option<f64>;
pub struct FrameStats { /* intervals_ms, cpu_ms ring buffers, frames */ }
impl FrameStats {
    pub fn new() -> FrameStats;
    /// Call at the start of each `ui()`; records the interval since the previous call.
    pub fn frame_started(&mut self, now: std::time::Instant);
    pub fn cpu_finished(&mut self, cpu: std::time::Duration);
    pub fn frames(&self) -> u64;
    pub fn interval_p99(&self) -> Option<f64>;
    pub fn cpu_p99(&self) -> Option<f64>;
}

// bench.rs
pub const DURATION_S: f64 = 10.0;
/// Camera for `t` seconds into the benchmark: 5 s of panning a 2000-unit circle, then 5 s
/// of zooming between 0.5 and 2 about the view center. `None` once `t >= DURATION_S`.
pub fn camera_at(t: f64, start: Camera, view_size: [f64; 2]) -> Option<Camera>;

// fixture.rs
/// A deterministic scene: 45% rectangles/diamonds/ellipses, 25% lines/arrows, 15% freedraw,
/// 10% text, 5% at opacity 50; every fill style; spread over 4000 x 3000 units.
pub fn generate(seed: u32, count: usize) -> scene::SceneFile;
```

- [ ] **Step 1：測試**

```rust
// stats.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentile() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&samples, 99.0), Some(99.0));
        assert_eq!(percentile(&samples, 100.0), Some(100.0));
        assert_eq!(percentile(&[], 99.0), None);
    }

    #[test]
    fn intervals_start_on_the_second_frame() {
        let start = std::time::Instant::now();
        let mut stats = FrameStats::new();
        stats.frame_started(start);
        assert_eq!(stats.interval_p99(), None);
        stats.frame_started(start + std::time::Duration::from_millis(8));
        assert_eq!(stats.frames(), 2);
        assert_eq!(stats.interval_p99(), Some(8.0));
    }
}

// bench.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_covers_pan_then_zoom_then_stops() {
        let start = Camera::default();
        let view = [800.0, 600.0];
        let pan = camera_at(2.5, start, view).expect("panning");
        assert_eq!(pan.zoom, 1.0);
        assert!(pan.scroll_x != 0.0 || pan.scroll_y != 0.0);
        let zoom = camera_at(7.5, start, view).expect("zooming");
        assert!((0.5..=2.0).contains(&zoom.zoom));
        assert_eq!(camera_at(DURATION_S, start, view), None);
    }
}

// fixture.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_typed() {
        let a = generate(7, 1000);
        assert_eq!(a.elements.len(), 1000);
        assert_eq!(a.to_json_string(), generate(7, 1000).to_json_string());
        assert!(a.elements.iter().all(|e| !matches!(e, scene::Element::Raw(_))));
        let translucent = a.elements.iter().filter(|e| e.opacity() < 100.0).count();
        assert!((30..=70).contains(&translucent), "{translucent}");
    }
}
```

- [ ] **Step 2：確認失敗，然後實作**

Run: `cargo test -p app --lib stats bench fixture`
Expected: 編譯失敗。

- `fixture::generate`：LCG（`state = state * 1664525 + 1013904223`，取 `u32`）決定型別、位置、大小、填色樣式、顏色；元件用 `sample.rs` 的 JSON 建立（`seed` 也來自 LCG，id 用 `e{i}`），`index` 用 `generate_n_keys_between(None, None, count)`；`SceneFile` 由 `sample::file` 組成。
- `examples/perf_fixture.rs`：`cargo run -p app --example perf_fixture -- OUT.excalidraw [SEED]`，預設 seed 1、1000 個元件，寫出 `to_json_string()`。
- `Viewer`：每幀 `frame_started`，`ui()` 結束時 `cpu_finished`（包含 paint callback 的 `prepare` 時間：在 `CanvasRenderer` 內量測 prepare 並經 `RenderStats` 回報，加總後記錄）。`F12` 切換右下角面板，顯示 frames、interval p99、cpu p99、drawn elements、cached meshes、buffer vertices。`--bench`：第一個畫面後開始，每幀以 `camera_at` 設相機並 `request_repaint()`；結束時印出 `bench: frames {n}, interval p99 {x:.2} ms, cpu p99 {y:.2} ms` 並 `send_viewport_cmd(ViewportCommand::Close)`。

- [ ] **Step 3：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

效能驗收（spec §6.7，決定 11）：

```bash
cargo run --release -p app --example perf_fixture -- /tmp/napkin-perf.excalidraw
cargo run --release -p app -- /tmp/napkin-perf.excalidraw --bench
```

Expected：`interval p99` 小於 12.5 ms。視窗在 Hyprland 以 70%×70% 浮動大小測（`hyprctl dispatch togglefloating` 後 `resizeactive exact 70% 70%`）。另外手動確認：開啟後不操作 5 秒，面板的 frames 不增加。

不達標時先用面板找原因（cpu p99 高代表 CPU 端；cpu 低而 interval 高代表 GPU 端或 draw call 太多），修到達標再 commit；量到的數字寫進 commit message。

- [ ] **Step 4：Commit**

```bash
git add crates/app
git commit -m "Add frame-time panel, perf fixture and --bench mode"
```

commit message 內文記錄 `--bench` 的輸出與機器（Intel Arc B390、2880×1800 @ 120Hz、scale 1.25）。

---

### Task 9：觸控板捏合縮放

**Files:**
- Create: `crates/app/src/pinch.rs`
- Modify: `Cargo.toml`（workspace dependencies 加 `raw-window-handle`、`wayland-backend`（`client_system`）、`wayland-client`、`wayland-protocols`（`client`、`unstable`）、`rustix`（`event`））、`crates/app/Cargo.toml`、`crates/app/src/viewer.rs`、`crates/app/src/lib.rs`

**Interfaces:**
- Consumes: Task 3 `CanvasInput::pinch`；M0 `pinch_probe.rs`（commit `7e14330`）。
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinchEvent { Begin, Update { scale: f64 }, End }

/// Turns the protocol's begin-relative `scale` into per-event zoom factors.
#[derive(Debug)]
pub struct PinchTracker { previous: f64 }
/// Starts as if a gesture had just begun (`previous` = 1), so an `Update` that arrives
/// without a `Begin` still yields a finite factor.
impl Default for PinchTracker { /* previous: 1.0 */ }
impl PinchTracker {
    pub fn factor(&mut self, event: PinchEvent) -> Option<f64>;
}

pub struct PinchListener { /* receiver, eventfd, join handle */ }
impl PinchListener {
    /// `Err` when the display is not Wayland or the compositor lacks
    /// `zwp_pointer_gestures_v1`; the caller keeps Ctrl+wheel zoom only.
    pub fn start(cc: &eframe::CreationContext<'_>) -> Result<PinchListener, String>;
    pub fn events(&self) -> Vec<PinchEvent>;
    /// Wakes the dispatch thread and joins it. Idempotent.
    pub fn stop(&mut self);
}
impl Drop for PinchListener { /* stop() */ }
```

- [ ] **Step 1：`PinchTracker` 的測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_becomes_per_event_factors() {
        let mut tracker = PinchTracker::default();
        assert_eq!(tracker.factor(PinchEvent::Begin), None);
        assert_eq!(tracker.factor(PinchEvent::Update { scale: 1.2 }), Some(1.2));
        let factor = tracker.factor(PinchEvent::Update { scale: 1.5 }).expect("factor");
        assert!((factor - 1.25).abs() < 1e-12);
        assert_eq!(tracker.factor(PinchEvent::End), None);
        assert_eq!(tracker.factor(PinchEvent::Begin), None);
        assert_eq!(tracker.factor(PinchEvent::Update { scale: 0.8 }), Some(0.8));
    }
}
```

Run: `cargo test -p app --lib pinch`
Expected: 編譯失敗；實作 `PinchTracker` 後通過。

- [ ] **Step 2：實作 `PinchListener`**

從 `pinch_probe.rs` 取綁定流程（`Backend::from_foreign_display` → `Connection::from_backend` → `registry_queue_init` → 綁 `ZwpPointerGesturesV1` 與 `wl_seat` → seat 有 pointer 能力時 `get_pointer` + `get_pinch_gesture`），改動：

- 手勢事件送進 `mpsc::Sender<PinchEvent>` 並 `ctx.request_repaint()`。
- 專用執行緒不用 `blocking_dispatch`，改成可以停止的迴圈：

```rust
loop {
    event_queue.flush().ok();
    let Some(guard) = event_queue.prepare_read() else {
        if event_queue.dispatch_pending(&mut state).is_err() { break; }
        continue;
    };
    let ready = {
        let fd = guard.connection_fd();
        let mut fds = [
            rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&stop, rustix::event::PollFlags::IN),
        ];
        if rustix::event::poll(&mut fds, None).is_err() { break; }
        if fds[1].revents().contains(rustix::event::PollFlags::IN) { break; }
        fds[0].revents().contains(rustix::event::PollFlags::IN)
    };
    if ready && guard.read().is_err() { break; }
    if event_queue.dispatch_pending(&mut state).is_err() { break; }
}
```

（`guard` 在 `break` 時被 drop，等於放棄這次讀取，符合 libwayland 的 `prepare_read`／`cancel_read` 規則。）

- `stop`：對 eventfd 寫入 1，`join` 執行緒；之後 drop 自己的 `Connection` 與 proxy。`stop` 必須在 eframe 釋放 `wl_display` 之前完成：`Viewer::on_exit` 呼叫 `stop()`，`Drop` 再保險一次。
- 生命週期的理由寫在 `PinchListener` 的 doc comment：winit 的連線在 eframe 的事件迴圈結束時才 `wl_display_disconnect`，`on_exit` 在那之前執行，所以在 `on_exit` 停止執行緒後，外借的 `wl_display` 指標在整個執行緒存活期間都有效。
- `Viewer`：`new` 時 `PinchListener::start`，失敗就 `eprintln!` 原因並不啟用；每幀把 `events()` 經 `PinchTracker::factor` 相乘成 `CanvasInput::pinch`。

- [ ] **Step 3：驗證**

Run: `cargo test -p app` 然後 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通過。

手動：
1. 在 Hyprland 上開 `shapes.excalidraw`，觸控板捏合時畫布以游標位置為中心平順縮放，放開即停。
2. 捏合期間移動游標、點擊不受影響（M0 檢查 H）。
3. 關閉視窗 10 次（`SUPER+W` 與關閉按鈕各 5 次），每次都正常結束，沒有 segfault（`coredumpctl list napkin` 沒有新紀錄）。
4. `Ctrl`+滾輪縮放仍然有效。

- [ ] **Step 4：Commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "Zoom on touchpad pinch via zwp_pointer_gestures_v1"
```

---

## M3 完成條件

roadmap：人工驗收 #3，以及 spec §6.7 平移／縮放的影格時間目標。

1. Task 1 到 9 的自動測試全部通過。
2. spec §9.3 #3：三個語料檔（`crates/scene/tests/corpus/`）在 napkin 與 excalidraw.com 並排比對，線條形狀一致；淺色、深色主題各看一次。
3. Task 8 的 `--bench` 在 1000 個元件的測試場景上 interval p99 小於 12.5 ms，且閒置時不重繪。

## 留給後續里程碑

M2 審查時決定延後、這份計畫也不處理的項目，保留在這裡，因為 M2 的執行帳本不在 git 裡：

- **M4（編輯、存檔）**
  - `SceneFile` 寫回時 key 順序與原檔不同（spec §9.2 的語意比對忽略順序）。
  - `elements` 或 `appState` 為 `null` 的檔案載入失敗（不改檔，但 excalidraw.com 能開）。
  - `file.rs` 的 `AppStateNotObject` 與寫出分支沒有測試。
  - `Element::base_mut` 讓呼叫端能改 `kind`。
  - `fractional_index` 的 `midpoint` 遞迴深度隨尾端連續 `z` 的長度增加，`sync_invalid_indices` 還有一個 `expect`（兩者都已在程式碼旁註明）；M4 載入時同步 index 前處理。
  - 含孤立 UTF-16 surrogate 跳脫字元（例如 `"\ud83d"`）的檔案載入失敗，`JSON.parse` 卻接受（已在 `SceneFile::from_json_str` 註明）；M4 的錯誤訊息要考慮。
  - 新元件建構函數接受 `Some("")` 的箭頭頭部；`backgroundColor` 為 `""` 時被當成有填色。
  - `type: "line"` 卻帶 `elbowed: true` 的手改檔案會畫成 elbow 路徑。
- **下次重新產生基準時補的案例**（`tools/baseline/scene/cases.mjs`）
  - `rough_options`：line／freedraw 的 `!= "transparent"` 與 `isTransparent` 差異、`.min(2.5)`、10／15 的尺寸門檻、未知的 `strokeStyle`、沒有 `roundness`。
  - `shapes_generic`：圓角半徑 fallback 0、`roundness.value` 為 `null`、超過 cutoff 的 adaptive 半徑、負的或非整數的圓角尺寸、指數表示法的 path 數字。
  - `freedraw_outline`：沒有 `simulatePressure`、`simulatePressure: false` 且 points 為空、`variability` 為 `null` 或未知、`strokeOptions` 為 `null`、`streamline` 為 `null`；以及語料 `shapes.excalidraw` 裡真實滑鼠畫的 freedraw 筆畫。
  - 產生器輸出的 `source` 字串補上 `points-on-curve`；`syncMovedIndices` 與 `syncInvalidIndices` 的摘要補 `versionNonce`、`updated`。
  - 產生器在 git fetch 失敗時留下半成品 `.cache/.git`，下次執行只在 `rev-parse` 失敗，訊息不清楚。
