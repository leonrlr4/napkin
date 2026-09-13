# napkin M0：風險驗證 Implementation Plan

> Historical record, frozen 2026-09-13. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在寫正式程式碼之前，確認 spec §10 的每一項技術風險該走主方案還是替代方案，並把正式要用的字型建置工具放進 repo。

**Architecture:** 兩支丟棄式的實驗程式放在 `spikes/m0/`（獨立的 Cargo package，不屬於之後的 workspace）。驗證結果寫進 `docs/decisions/napkin-m0-findings.md`，寫完後刪除實驗程式。字型建置工具是正式工具，放在 `tools/fonts/`，產出到 `assets/fonts/`。

**Tech Stack:** Rust 1.98.1（edition 2024）、eframe／egui-wgpu 0.36.2、glyphon 0.12.0、wgpu 30；Python 3.12+、uv、fonttools 4.65.0；量測工具 ImageMagick。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（§10 風險表、§6.3 字型）
**Roadmap:** `docs/decisions/plans/2026-09-13-napkin-roadmap.md`

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- Excalidraw 基準 commit：`afa3a653fc5d2b742adcbd5a6063187b056d2419`。
- 版本鎖定：`eframe = "=0.36.2"`、`egui-wgpu = "=0.36.2"`、`glyphon = "=0.12.0"`、`wgpu = "30"`、`fonttools[woff]==4.65.0`。eframe、egui-wgpu、glyphon 共用 wgpu 30 和 winit 0.30，**不能只升級其中一個**。
- 系統中文字型：`/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc`，其中 "Noto Sans CJK TC" 的 face index 是 `3`。
- `spikes/m0/` 的程式碼是丟棄式的，Task 4 會刪除。

## 寫計畫時已經完成的驗證

- 本計畫裡的 `input_probe.rs`、`canvas_probe.rs` 已經在上面鎖定的版本下實際編譯過，沒有任何 warning。**但還沒有執行過**，執行結果正是本計畫要找出來的答案。
- 本計畫裡的 `build_fonts.py` 已經實際執行過，結果是：`napkin-hand` 7 個子集合併成 561 個 codepoint、`napkin-sans` 5 個子集 854 個、`napkin-code` 4 個子集 333 個，**全部 0 mismatches**。`test_build_fonts.py` 3 項全部通過。另外做過一次突變測試：把字形比對拿掉之後，「對應到錯誤字形」那項測試確實會失敗。

---

### Task 1：輸入實驗（fcitx5 中文輸入、觸控板捏合）

**Files:**
- Create: `.gitignore`
- Create: `spikes/m0/Cargo.toml`
- Create: `spikes/m0/src/bin/input_probe.rs`
- Create: `docs/decisions/napkin-m0-findings.md`

**Interfaces:**
- Consumes: 無
- Produces: `spikes/m0` package（Task 2 會在同一個 package 加上 `canvas_probe`）；findings 文件的骨架（Task 2、3、4 會填入各自的段落）

- [ ] **Step 1：建立 `.gitignore`**

```gitignore
target/
/spikes/m0/out/
```

- [ ] **Step 2：建立 `spikes/m0/Cargo.toml`**

```toml
[package]
name = "napkin-spike-m0"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
bytemuck = { version = "1", features = ["derive"] }
eframe = { version = "=0.36.2", default-features = false, features = ["default_fonts", "wayland", "wgpu"] }
egui-wgpu = "=0.36.2"
glyphon = "=0.12.0"
wgpu = "30"
```

- [ ] **Step 3：建立 `spikes/m0/src/bin/input_probe.rs`**

```rust
//! M0 spike (throwaway): does fcitx5 reach an egui `TextEdit` on Hyprland, and does
//! winit deliver touchpad pinch as `egui::Event::Zoom`? Findings are recorded in
//! docs/decisions/; this file is deleted once they are.

use std::sync::Arc;

use eframe::egui;

const CJK_FONT_PATH: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";
/// Face index of "Noto Sans CJK TC" inside the collection (`fc-query` on this machine).
const CJK_FONT_INDEX: u32 = 3;
const LOG_LINES: usize = 24;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin-spike")
            .with_inner_size([900.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "napkin spike: input",
        options,
        Box::new(|cc| {
            install_cjk_font(&cc.egui_ctx);
            Ok(Box::new(InputProbe::default()))
        }),
    )
}

fn install_cjk_font(ctx: &egui::Context) {
    let bytes = std::fs::read(CJK_FONT_PATH).expect("Noto Sans CJK is required for this probe");
    let mut data = egui::FontData::from_owned(bytes);
    data.index = CJK_FONT_INDEX;
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("noto-cjk".to_owned(), Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .get_mut(&family)
            .expect("egui defines both default families")
            .push("noto-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[derive(Default)]
struct InputProbe {
    text: String,
    log: Vec<String>,
}

impl InputProbe {
    fn record(&mut self, line: String) {
        println!("{line}");
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            self.log.remove(0);
        }
    }
}

impl eframe::App for InputProbe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Ime(ime) => self.record(format!("IME  {ime:?}")),
                egui::Event::Zoom(factor) => self.record(format!("ZOOM {factor}")),
                _ => {}
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("1. Click the box, switch fcitx5 to Chinese, type nihao + space");
            // Offset from the window origin so a candidate popup stuck at (0,0) is obvious.
            ui.add_space(160.0);
            ui.horizontal(|ui| {
                ui.add_space(320.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .font(egui::TextStyle::Heading)
                        .desired_width(320.0),
                );
            });
            ui.add_space(24.0);
            ui.heading("2. Pinch on the touchpad anywhere in this window");
            ui.separator();
            for line in &self.log {
                ui.monospace(line);
            }
        });
    }
}
```

- [ ] **Step 4：編譯**

Run: `cargo build --manifest-path spikes/m0/Cargo.toml --bin input_probe 2>&1 | grep -E '^(warning|error)|Finished'`
Expected: 只有一行 `Finished ...`，沒有任何 `warning` 或 `error`。第一次編譯會編 wgpu，需要幾分鐘。

- [ ] **Step 5：建立 findings 文件骨架 `docs/decisions/napkin-m0-findings.md`**

````markdown
# napkin M0 風險驗證結果

> Historical record, frozen FROZEN_DATE. Source code is authoritative; where this
> document and the code disagree, the code wins.

**Plan:** `docs/decisions/plans/2026-09-13-napkin-m0-risk-validation.md`
**Spec 風險表:** `docs/decisions/specs/2026-09-13-napkin-design.md` §10
**實驗程式碼（已刪除，可從 git 歷史取回）:** SPIKE_COMMITS

## 1. fcitx5 中文輸入（input_probe）

| 檢查 | 結果 | 觀察 |
|---|---|---|
| A. 組字中的內容（preedit）顯示在文字框裡 | | |
| B. 候選字視窗出現在文字框附近 | | |
| C. 選字後文字框出現「你好」 | | |
| D. log 出現 `IME  Preedit` 與 `IME  Commit` | | |

**結論：**

## 2. 觸控板捏合縮放（input_probe）

| 檢查 | 結果 | 觀察 |
|---|---|---|
| E. 捏合時 log 出現 `ZOOM` | | |

**結論：**

## 3. MSAA（canvas_probe）

| 檢查 | 結果 | 數值 |
|---|---|---|
| 程式沒有 panic | | |
| 斜線裁切區的顏色數 ≥ 3 | | |
| adapter backend | — | |

**結論：**

## 4. glyphon 與圖形交錯繪製（canvas_probe）

| 檢查 | 結果 | 數值 |
|---|---|---|
| 紅框裁切區的顏色數 > 2（文字畫在紅框上） | | |
| 藍框裁切區的顏色數 = 1（文字被藍框蓋住） | | |

**結論：**

## 5. 字型子集合併（build_fonts.py）

| 字型 | 子集數 | codepoint 數 | mismatches |
|---|---|---|---|
| napkin-hand | | | |
| napkin-sans | | | |
| napkin-code | | | |

**結論：**

## 對後續里程碑的影響

````

- [ ] **Step 6：【需要使用者親自操作】執行並觀察**

這一步 agent 無法代勞：輸入法的組字需要實體鍵盤。請使用者執行：

```bash
mkdir -p spikes/m0/out
spikes/m0/target/debug/input_probe | tee spikes/m0/out/input_probe.log
```

視窗出現後依序操作：

1. 點一下文字框，把 fcitx5 切到中文，輸入 `nihao`，**先不要按空白鍵**。
   - A. 文字框裡看得到正在組字的內容
   - B. 候選字視窗出現在文字框附近，而不是在視窗或螢幕的左上角
2. 按空白鍵選字。
   - C. 文字框裡出現「你好」
   - D. 視窗下方的 log 出現 `IME  Preedit { ... }` 和 `IME  Commit("你好")`
3. 在視窗內用觸控板雙指捏合。
   - E. log 出現以 `ZOOM` 開頭的行

關閉視窗。

- [ ] **Step 7：判定並填入 findings 第 1、2 節**

根據使用者回報的 A–E，以及 `spikes/m0/out/input_probe.log` 的內容填表。「結論」照下表寫：

| 結果 | 第 1 節結論 |
|---|---|
| A、B、C、D 全部 PASS | 主方案：文字編輯使用 egui `TextEdit` 疊在元件上（spec §7.3） |
| D PASS，但 A、B、C 有任一 FAIL | 替代方案：自建文字編輯元件，直接處理 `egui::Event::Ime` |
| D FAIL（log 完全沒有 IME 事件） | **停止**：IME 在 winit 層就不通。Task 2、3 照常執行（它們的結果仍然有用），但 Task 4 回報時要明確標示 M3 以後暫停，回到設計討論 |

| 結果 | 第 2 節結論 |
|---|---|
| E PASS | 支援觸控板捏合縮放 |
| E FAIL | v1 只支援 `Ctrl`+滾動縮放 |

- [ ] **Step 8：Commit**

```bash
git add .gitignore spikes/m0/Cargo.toml spikes/m0/src/bin/input_probe.rs docs/decisions/napkin-m0-findings.md
git commit -m "Add M0 input probe and record IME and pinch findings"
```

---

### Task 2：畫布實驗（MSAA、glyphon 與圖形交錯繪製）

**Files:**
- Create: `spikes/m0/src/bin/canvas_probe.rs`
- Modify: `docs/decisions/napkin-m0-findings.md`（第 3、4 節）

**Interfaces:**
- Consumes: Task 1 的 `spikes/m0/Cargo.toml`（依賴已經包含 `bytemuck`、`egui-wgpu`、`glyphon`、`wgpu`）
- Produces: findings 第 3、4 節

- [ ] **Step 1：建立 `spikes/m0/src/bin/canvas_probe.rs`**

```rust
//! M0 spike (throwaway): inside a single egui_wgpu paint callback, is MSAA active, and
//! can glyphon text be drawn *between* two custom mesh draws (shape, text, shape)?
//! Findings are recorded in docs/decisions/; this file is deleted once they are.

use eframe::egui;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};

const MSAA_SAMPLES: u32 = 4;
/// The first two quads (diagonal line, red box) are drawn before the text.
const VERTICES_UNDER_TEXT: u32 = 12;
/// The third quad (blue box) is drawn after the text and must hide part of it.
const VERTICES_TOTAL: u32 = 18;

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct CanvasResources {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    font_system: FontSystem,
    swash_cache: SwashCache,
    _glyph_cache: Cache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    label: Buffer,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin-spike")
            .with_inner_size([1000.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        multisampling: MSAA_SAMPLES as u16,
        ..Default::default()
    };
    eframe::run_native(
        "napkin spike: canvas",
        options,
        Box::new(|cc| {
            let render_state = cc
                .wgpu_render_state
                .as_ref()
                .expect("eframe must run with the wgpu renderer");
            println!("adapter: {:?}", render_state.adapter.get_info());
            println!(
                "target format: {:?}, requested MSAA: {MSAA_SAMPLES}",
                render_state.target_format
            );
            let resources = create_resources(render_state);
            render_state
                .renderer
                .write()
                .callback_resources
                .insert(resources);
            Ok(Box::new(CanvasProbe::default()))
        }),
    )
}

fn create_resources(render_state: &egui_wgpu::RenderState) -> CanvasResources {
    let device = &render_state.device;
    let multisample = wgpu::MultisampleState {
        count: MSAA_SAMPLES,
        ..Default::default()
    };

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spike shapes"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spike shapes"),
        ..Default::default()
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("spike shapes"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: render_state.target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample,
        multiview_mask: None,
        cache: None,
    });
    let vertices = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spike shapes"),
        size: u64::from(VERTICES_TOTAL) * size_of::<Vertex>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut font_system = FontSystem::new();
    let glyph_cache = Cache::new(device);
    let viewport = Viewport::new(device, &glyph_cache);
    let mut atlas = TextAtlas::new(
        device,
        &render_state.queue,
        &glyph_cache,
        render_state.target_format,
    );
    let text_renderer = TextRenderer::new(&mut atlas, device, multisample, None);
    let mut label = Buffer::new(&mut font_system, Metrics::new(64.0, 80.0));
    label.set_text(
        "Hello 手繪白板 napkin",
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        None,
    );

    CanvasResources {
        pipeline,
        vertices,
        font_system,
        swash_cache: SwashCache::new(),
        _glyph_cache: glyph_cache,
        viewport,
        atlas,
        text_renderer,
        label,
    }
}

/// Where the probe writes its own screenshot. egui captures the frame on the GPU before
/// the compositor sees it, so Hyprland's window opacity and blur cannot skew the pixels.
const SCREENSHOT_PATH: &str = "spikes/m0/out/canvas.ppm";
/// Hyprland may resize the window right after mapping it; wait for the layout to settle.
const STABLE_FRAMES_BEFORE_SCREENSHOT: u32 = 30;

#[derive(Default)]
struct CanvasProbe {
    reported_rect_px: Option<[u32; 4]>,
    stable_frames: u32,
    screenshot_requested: bool,
}

impl eframe::App for CanvasProbe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let screenshot = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = screenshot {
            write_ppm(SCREENSHOT_PATH, &image).expect("write screenshot");
            println!("screenshot saved to {SCREENSHOT_PATH}");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(
                "Expect: a smooth thin diagonal line, the text on top of the red box, \
                 and the blue box hiding the right half of the text.",
            );
            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
            let ppp = ctx.pixels_per_point();
            let rect_px =
                [rect.min.x, rect.min.y, rect.width(), rect.height()].map(|v| (v * ppp) as u32);
            if self.reported_rect_px == Some(rect_px) {
                self.stable_frames += 1;
            } else {
                let [x, y, w, h] = rect_px;
                println!("callback rect px {x} {y} {w} {h}");
                self.reported_rect_px = Some(rect_px);
                self.stable_frames = 0;
            }
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                CanvasCallback {
                    size_px: [rect.width() * ppp, rect.height() * ppp],
                },
            ));
        });

        if !self.screenshot_requested {
            if self.stable_frames >= STABLE_FRAMES_BEFORE_SCREENSHOT {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.screenshot_requested = true;
            }
            // egui only repaints on input; keep frames coming until the capture is taken.
            ctx.request_repaint();
        }
    }
}

fn write_ppm(path: &str, image: &egui::ColorImage) -> std::io::Result<()> {
    let [width, height] = image.size;
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in &image.pixels {
        bytes.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
    }
    std::fs::write(path, bytes)
}

struct CanvasCallback {
    size_px: [f32; 2],
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let res: &mut CanvasResources = callback_resources
            .get_mut()
            .expect("resources are inserted at startup");
        let [w, h] = self.size_px;

        queue.write_buffer(
            &res.vertices,
            0,
            bytemuck::cast_slice(&scene_vertices(w, h)),
        );

        res.viewport.update(
            queue,
            Resolution {
                width: w as u32,
                height: h as u32,
            },
        );
        res.label.set_size(Some(w), Some(h));
        res.label.shape_until_scroll(&mut res.font_system, false);
        res.text_renderer
            .prepare(
                device,
                queue,
                &mut res.font_system,
                &mut res.atlas,
                &res.viewport,
                [TextArea {
                    buffer: &res.label,
                    left: w * 0.10,
                    top: h * 0.44,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: w as i32,
                        bottom: h as i32,
                    },
                    default_color: Color::rgb(240, 240, 240),
                    custom_glyphs: &[],
                }],
                &mut res.swash_cache,
            )
            .expect("glyphon prepare");
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let res: &CanvasResources = callback_resources
            .get()
            .expect("resources are inserted at startup");

        render_pass.set_pipeline(&res.pipeline);
        render_pass.set_vertex_buffer(0, res.vertices.slice(..));
        render_pass.draw(0..VERTICES_UNDER_TEXT, 0..1);

        res.text_renderer
            .render(&res.atlas, &res.viewport, render_pass)
            .expect("glyphon render");

        // glyphon leaves its own pipeline and bind groups bound; restore ours.
        render_pass.set_pipeline(&res.pipeline);
        render_pass.set_vertex_buffer(0, res.vertices.slice(..));
        render_pass.draw(VERTICES_UNDER_TEXT..VERTICES_TOTAL, 0..1);
    }
}

/// Positions are laid out in physical pixels relative to the callback rect, then mapped
/// to NDC: egui sets the render pass viewport to that rect before calling `paint`.
fn scene_vertices(w: f32, h: f32) -> Vec<Vertex> {
    let ndc = |x: f32, y: f32| [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0];
    let quad = |a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2], color: [f32; 4]| {
        [a, b, c, a, c, d].map(|p| Vertex {
            position: ndc(p[0], p[1]),
            color,
        })
    };

    // A 1.5px-wide nearly diagonal line: jagged without MSAA, smooth with it.
    let (x0, y0, x1, y1) = (w * 0.05, h * 0.92, w * 0.95, h * 0.08);
    let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    let (nx, ny) = (-(y1 - y0) / len * 0.75, (x1 - x0) / len * 0.75);
    let line = quad(
        [x0 + nx, y0 + ny],
        [x1 + nx, y1 + ny],
        [x1 - nx, y1 - ny],
        [x0 - nx, y0 - ny],
        [0.9, 0.9, 0.9, 1.0],
    );
    let rect = |l: f32, t: f32, r: f32, b: f32, color| quad([l, t], [r, t], [r, b], [l, b], color);
    let red_under = rect(w * 0.08, h * 0.40, w * 0.50, h * 0.62, [0.8, 0.2, 0.2, 1.0]);
    let blue_over = rect(
        w * 0.45,
        h * 0.38,
        w * 0.92,
        h * 0.64,
        [0.2, 0.35, 0.85, 1.0],
    );

    [line, red_under, blue_over].concat()
}
```

- [ ] **Step 2：編譯**

Run: `cargo build --manifest-path spikes/m0/Cargo.toml --bin canvas_probe 2>&1 | grep -E '^(warning|error)|Finished'`
Expected: 只有一行 `Finished ...`。

- [ ] **Step 3：執行（程式會自己截圖並結束）**

```bash
mkdir -p spikes/m0/out
timeout 60 spikes/m0/target/debug/canvas_probe 2>&1 | tee spikes/m0/out/canvas_probe.log
echo "exit=${PIPESTATUS[0]}"
```

Expected（正常情況）：log 依序出現 `adapter: AdapterInfo { ... }`、`target format: ...`、一行以上的 `callback rect px X Y W H`、`screenshot saved to spikes/m0/out/canvas.ppm`，最後是 `exit=0`。視窗會出現大約一秒後自動關閉。

截圖是用 egui 的 `ViewportCommand::Screenshot` 從 GPU 讀回來的最終畫面，**不是**用 grim 截取合成後的螢幕。omarchy 會替所有視窗套用 `default-opacity`（這台設定是 0.85），用 grim 截圖會混入視窗背後的內容，顏色數就不準了。

如果 log 裡有 panic：記下 panic 訊息，直接跳到 Step 5 判定。wgpu 發現 pipeline 和 render pass 的 sample count 不一致時會 panic，這本身就是 MSAA 的答案。`exit=124` 代表 60 秒內沒有截到圖，一樣把 log 的最後幾行記下來，視為 FAIL。

- [ ] **Step 4：量測三個裁切區的顏色數**

截圖和 `callback rect px` 都是視窗內的實體像素，可以直接換算：

```bash
magick spikes/m0/out/canvas.ppm spikes/m0/out/canvas.png
read RX RY RW RH <<< "$(grep '^callback rect px' spikes/m0/out/canvas_probe.log | tail -1 | cut -d' ' -f4-)"
crop() {  # crop <x%> <y%> <w%> <h%> <out.png>: region relative to the callback rect, prints unique colours
  magick spikes/m0/out/canvas.png \
    -crop "$(( RW * $3 / 100 ))x$(( RH * $4 / 100 ))+$(( RX + RW * $1 / 100 ))+$(( RY + RH * $2 / 100 ))" \
    +repage "$5"
  magick "$5" -format '%k\n' info:
}
echo "line:"; crop 6 68 24 24 spikes/m0/out/line.png
echo "red:";  crop 12 45 32 9  spikes/m0/out/red.png
echo "blue:"; crop 50 42 38 18 spikes/m0/out/blue.png
```

各裁切區的意義：
- **line**：只包含背景和斜線。有 MSAA 時，邊緣會有介於兩色之間的像素，所以顏色數 ≥ 3；沒有 MSAA 只會是 2。
- **red**：紅框內部，文字畫在紅框之後。文字有畫出來的話，顏色數 > 2。
- **blue**：藍框內部，藍框畫在文字之後。交錯順序正確的話，文字完全被蓋住，顏色數 = 1。

接著用 Read 工具打開 `spikes/m0/out/canvas.png`（以及 `line.png`、`red.png`、`blue.png`）目視確認：斜線平滑、文字壓在紅框上、文字右半部被藍框蓋住。如果數字和目視結果不一致，以目視結果為準，並在「數值」欄寫明原因。

- [ ] **Step 5：判定並填入 findings 第 3、4 節**

| 結果 | 第 3 節結論 |
|---|---|
| 沒有 panic，且 line ≥ 3 | 主方案：整個視窗使用 MSAA 4×（`NativeOptions::multisampling = 4`） |
| panic 訊息提到 sample count，或 line = 2 | 替代方案：畫布先畫到自己的 4× MSAA 離屏 texture，resolve 後再合成 |

| 結果 | 第 4 節結論 |
|---|---|
| red > 2 且 blue = 1 | 主方案：glyphon 文字以「連續圖形一組、連續文字一組」的方式交錯繪製 |
| red ≤ 2（文字沒有畫出來）或 blue > 1（文字蓋在藍框上） | 替代方案：用 cosmic-text `SwashCache` 自建字形 atlas，字形 quad 併入畫布的 vertex stream |

adapter 那一列填 log 裡 `AdapterInfo` 的 `backend` 值。預期是 `Vulkan`；如果不是，在「觀察」寫明，因為這會影響 spec §6.7 的效能目標。

- [ ] **Step 6：Commit**

```bash
git add spikes/m0/src/bin/canvas_probe.rs docs/decisions/napkin-m0-findings.md
git commit -m "Add M0 canvas probe and record MSAA and text interleaving findings"
```

---

### Task 3：字型建置工具與字型檔

**Files:**
- Create: `tools/fonts/test_build_fonts.py`
- Create: `tools/fonts/build_fonts.py`
- Create（由工具產生）: `assets/fonts/napkin-hand.ttf`、`assets/fonts/napkin-hand.LICENSE.txt`、`assets/fonts/napkin-sans.ttf`、`assets/fonts/napkin-sans.LICENSE.txt`、`assets/fonts/napkin-code.ttf`、`assets/fonts/napkin-code.LICENSE.txt`
- Modify: `docs/decisions/napkin-m0-findings.md`（第 5 節）

**Interfaces:**
- Consumes: 無（需要網路，會存取 GitHub API 與 raw.githubusercontent.com）
- Produces: 三個字型檔。family 名稱分別是 `napkin-hand`、`napkin-sans`、`napkin-code`，M3 的文字層會依 spec §6.3 的 `fontFamily` 對應表使用；`build_fonts.mismatches(subsets, merged) -> list[str]`

- [ ] **Step 1：先寫測試 `tools/fonts/test_build_fonts.py`**

```python
"""Guards the merge check in build_fonts.py: it must reject a bad merge, not just pass.

Runs against the committed assets/fonts, so it needs no network:
    uv run --with 'fonttools[woff]==4.65.0' --with pytest pytest tools/fonts
"""

import importlib.util
from pathlib import Path

from fontTools.ttLib import TTFont

_spec = importlib.util.spec_from_file_location(
    "build_fonts", Path(__file__).resolve().parent / "build_fonts.py"
)
build_fonts = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(build_fonts)

# napkin-code is monospaced, so every glyph has the same advance width: a wrong-glyph
# mapping is only detectable through the outline bounds, which is what the test needs.
FONT = build_fonts.OUT_DIR / "napkin-code.ttf"


def unicode_cmaps(font: TTFont) -> list[dict[int, str]]:
    return [table.cmap for table in font["cmap"].tables if table.isUnicode()]


def test_identical_fonts_have_no_mismatches():
    assert build_fonts.mismatches([TTFont(FONT)], TTFont(FONT)) == []


def test_missing_codepoint_is_reported():
    reference, broken = TTFont(FONT), TTFont(FONT)
    for cmap in unicode_cmaps(broken):
        cmap.pop(ord("A"), None)

    assert "U+0041 missing from merged font" in build_fonts.mismatches([reference], broken)


def test_codepoint_mapped_to_wrong_glyph_is_reported():
    reference, broken = TTFont(FONT), TTFont(FONT)
    wrong_glyph = reference.getBestCmap()[ord("W")]
    for cmap in unicode_cmaps(broken):
        if ord("B") in cmap:
            cmap[ord("B")] = wrong_glyph

    errors = build_fonts.mismatches([reference], broken)

    assert any(error.startswith("U+0042 glyph differs") for error in errors)
```

- [ ] **Step 2：執行測試，確認會失敗**

Run: `uv run --quiet --with 'fonttools[woff]==4.65.0' --with pytest pytest -q tools/fonts`
Expected: FAIL（collection error），原因是 `build_fonts.py` 還不存在（`FileNotFoundError`）。

- [ ] **Step 3：建立 `tools/fonts/build_fonts.py`**

```python
#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["fonttools[woff]==4.65.0"]
# ///
"""Build napkin's bundled fonts from the woff2 subsets in a pinned Excalidraw commit.

Excalidraw ships each font split into unicode-range woff2 subsets and no full upstream
font is published, so this merges the subsets into one TTF per font.

The merged fonts are renamed napkin-hand / napkin-sans / napkin-code. Excalifont's name
table says "Excalifont is a trademark of Excalidraw" and OFL grants no trademark rights,
so a modified build must not present itself under that name; the other two follow the
same rule for uniformity. `.excalidraw` files store a numeric fontFamily, not a name, so
renaming does not affect file compatibility.

Run from anywhere: `tools/fonts/build_fonts.py`. Writes assets/fonts/ and exits non-zero
if any codepoint of any subset is missing from, or maps to a different glyph in, the
merged font.
"""

import io
import json
import sys
import tempfile
import urllib.request
from pathlib import Path

from fontTools.merge import Merger
from fontTools.ttLib import TTFont

EXCALIDRAW_COMMIT = "afa3a653fc5d2b742adcbd5a6063187b056d2419"
FONTS_PATH = "packages/excalidraw/fonts"
OUT_DIR = Path(__file__).resolve().parents[2] / "assets" / "fonts"

# (directory under FONTS_PATH, napkin family name, license of the source font)
FONTS = [
    ("Excalifont", "napkin-hand", "OFL-1.1"),
    ("Nunito", "napkin-sans", "OFL-1.1"),
    ("ComicShanns", "napkin-code", "MIT"),
]

# Tables the merger cannot combine and napkin does not need. Nunito's STAT only describes
# its variable-font design axes, which a static merged instance no longer has.
DROPPED_TABLES = ("STAT",)


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "napkin-build-fonts"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def raw_url(path: str) -> str:
    return f"https://raw.githubusercontent.com/excalidraw/excalidraw/{EXCALIDRAW_COMMIT}/{path}"


def list_subsets(directory: str) -> list[str]:
    api = (
        "https://api.github.com/repos/excalidraw/excalidraw/contents/"
        f"{FONTS_PATH}/{directory}?ref={EXCALIDRAW_COMMIT}"
    )
    entries = json.loads(fetch(api))
    return sorted(e["path"] for e in entries if e["name"].endswith(".woff2"))


def ofl_text() -> str:
    """The full OFL 1.1 text, taken from the header comment of Excalifont's index.ts."""
    source = fetch(raw_url(f"{FONTS_PATH}/Excalifont/index.ts")).decode()
    start = source.index("license: ") + len("license: ")
    end = source.index("\nlicenseURL:", start)
    return source[start:end].strip() + "\n"


def decompress(woff2_path: str, workdir: Path) -> Path:
    font = TTFont(io.BytesIO(fetch(raw_url(woff2_path))))
    font.flavor = None
    for tag in DROPPED_TABLES:
        if tag in font:
            del font[tag]
    out = workdir / (Path(woff2_path).stem + ".ttf")
    font.save(out)
    return out


def rename(font: TTFont, family: str) -> None:
    postscript = f"{family}-Regular"
    names = {
        1: family,
        2: "Regular",
        3: f"{postscript};napkin",
        4: f"{family} Regular",
        6: postscript,
        16: family,
        17: "Regular",
    }
    table = font["name"]
    for name_id in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
        table.removeNames(nameID=name_id)
    for name_id, value in names.items():
        table.setName(value, name_id, 3, 1, 0x409)  # Windows, Unicode BMP, en-US
        table.setName(value, name_id, 1, 0, 0)  # Macintosh, Roman, English


def glyph_signature(font: TTFont, glyph: str) -> tuple:
    outline = font["glyf"][glyph]
    outline.recalcBounds(font["glyf"])
    bounds = tuple(getattr(outline, k, 0) for k in ("xMin", "yMin", "xMax", "yMax"))
    return font["hmtx"][glyph][0], bounds


def mismatches(subsets: list[TTFont], merged: TTFont) -> list[str]:
    merged_cmap = merged.getBestCmap()
    errors = []
    for subset in subsets:
        for codepoint, glyph in subset.getBestCmap().items():
            if codepoint not in merged_cmap:
                errors.append(f"U+{codepoint:04X} missing from merged font")
                continue
            expected = glyph_signature(subset, glyph)
            actual = glyph_signature(merged, merged_cmap[codepoint])
            if expected != actual:
                errors.append(f"U+{codepoint:04X} glyph differs: {expected} != {actual}")
    return errors


def license_text(directory: str, family: str, kind: str, source: TTFont, ofl: str) -> str:
    copyright_notice = source["name"].getDebugName(0) or ""
    provenance = (
        f"{family} is built by napkin's tools/fonts/build_fonts.py from the {directory}\n"
        f"woff2 subsets in Excalidraw commit {EXCALIDRAW_COMMIT}\n"
        f"({FONTS_PATH}/{directory}). Modifications: subsets merged into one font,\n"
        f"renamed to {family}.\n\n"
    )
    if kind == "OFL-1.1":
        return f"{provenance}{copyright_notice}\n\n{ofl}"
    # Comic Shanns carries its complete MIT license in the copyright name record.
    assert "Permission is hereby granted" in copyright_notice, f"{directory}: MIT text not found"
    return f"{provenance}{copyright_notice}\n"


def build(directory: str, family: str, kind: str, ofl: str) -> list[str]:
    with tempfile.TemporaryDirectory() as tmp:
        paths = [decompress(p, Path(tmp)) for p in list_subsets(directory)]
        subsets = [TTFont(p) for p in paths]
        merged = Merger().merge([str(p) for p in paths])
        rename(merged, family)
        out = OUT_DIR / f"{family}.ttf"
        merged.save(out)
        written = TTFont(out)
        errors = mismatches(subsets, written)
        (OUT_DIR / f"{family}.LICENSE.txt").write_text(
            license_text(directory, family, kind, subsets[0], ofl)
        )
        print(
            f"{family}: {len(paths)} subsets -> {out.name}, "
            f"{len(written.getBestCmap())} codepoints, {len(errors)} mismatches"
        )
        return errors


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    ofl = ofl_text()
    failed = False
    for directory, family, kind in FONTS:
        errors = build(directory, family, kind, ofl)
        for error in errors[:20]:
            print(f"  {error}")
        failed = failed or bool(errors)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
```

Run: `chmod +x tools/fonts/build_fonts.py`

- [ ] **Step 4：產生字型**

Run: `tools/fonts/build_fonts.py; echo "exit=$?"`
Expected（寫計畫時實際跑出來的結果）：

```
napkin-hand: 7 subsets -> napkin-hand.ttf, 561 codepoints, 0 mismatches
napkin-sans: 5 subsets -> napkin-sans.ttf, 854 codepoints, 0 mismatches
napkin-code: 4 subsets -> napkin-code.ttf, 333 codepoints, 0 mismatches
exit=0
```

- [ ] **Step 5：執行測試，確認通過**

Run: `uv run --quiet --with 'fonttools[woff]==4.65.0' --with pytest pytest -q tools/fonts`
Expected: `3 passed`

- [ ] **Step 6：確認改名生效**

Run: `for f in assets/fonts/*.ttf; do fc-query "$f" | grep -E '^\s+(family|postscriptname):' | tr -s '\t ' ' ' | paste -sd' '; done`
Expected:

```
 family: "napkin-code"(s)  postscriptname: "napkin-code-Regular"(s)
 family: "napkin-hand"(s)  postscriptname: "napkin-hand-Regular"(s)
 family: "napkin-sans"(s)  postscriptname: "napkin-sans-Regular"(s)
```

- [ ] **Step 7：填入 findings 第 5 節**

用 Step 4 的輸出填表。結論：mismatches 全為 0 時寫「主方案：合併成單一 TTF（spec §6.3）」；任何一個字型不為 0 時寫「替代方案：每個子集各自改名為獨立 family，由文字層依字元所在的 cmap 選擇」，並附上 mismatch 清單。

- [ ] **Step 8：Commit**

```bash
git add tools/fonts assets/fonts docs/decisions/napkin-m0-findings.md
git commit -m "Add font build tool and bundled napkin fonts"
```

---

### Task 4：完成 findings、刪除實驗程式

**Files:**
- Modify: `docs/decisions/napkin-m0-findings.md`
- Modify: `.gitignore`
- Delete: `spikes/m0/`

**Interfaces:**
- Consumes: Task 1–3 填好的 findings 第 1–5 節
- Produces: 定案的 findings 文件。之後寫 M3、M4 計畫時，依據它決定走主方案還是替代方案

- [ ] **Step 1：寫「對後續里程碑的影響」**

在 findings 最後一節，每個結論寫一行，格式是「里程碑：採用的方案」。對應關係：
- 第 1 節 → M4（文字編輯）
- 第 2 節 → M4（縮放手勢）
- 第 3 節 → M3（反鋸齒）
- 第 4 節 → M3（文字繪製）
- 第 5 節 → M3（字型載入）

如果第 1 節的結論是「停止」，這一節只寫「M1、M2 可以繼續；M3 以後暫停，等待設計討論」。

- [ ] **Step 2：填入實驗程式碼的 commit 與凍結日期**

```bash
SPIKES=$(git log --format='`%h`' -- spikes/m0 | paste -sd' ')
sed -i "s/SPIKE_COMMITS/${SPIKES}/; s/FROZEN_DATE/$(date +%F)/" docs/decisions/napkin-m0-findings.md
```

- [ ] **Step 3：檢查沒有漏填的欄位**

Run:

```bash
grep -nE '\| *\|$|\| *\| |結論：\*\*\s*$|FROZEN_DATE|SPIKE_COMMITS' docs/decisions/napkin-m0-findings.md
awk '/^## 對後續里程碑的影響/ { in_section = 1; next } in_section && NF { lines++ } END { print (lines > 0 ? "impact section ok" : "impact section EMPTY") }' docs/decisions/napkin-m0-findings.md
```

Expected: `grep` 沒有任何輸出，`awk` 印出 `impact section ok`。

- [ ] **Step 4：刪除實驗程式**

```bash
git rm -r -q spikes/m0
rm -rf spikes
sed -i '\#^/spikes/m0/out/$#d' .gitignore
```

Run: `cat .gitignore`
Expected: 只剩一行 `target/`

- [ ] **Step 5：Commit**

```bash
git add .gitignore docs/decisions/napkin-m0-findings.md
git commit -m "Record M0 findings and remove spike code"
```

- [ ] **Step 6：回報使用者**

用中文列出五項風險各自走哪個方案，並提醒下一步：寫 M1（rough）與 M2（scene）的實作計畫；M3 的計畫依本次 findings 撰寫。
