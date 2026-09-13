# napkin M0 風險驗證結果

> Historical record, frozen 2026-09-14. Source code is authoritative; where this
> document and the code disagree, the code wins.

**Plan:** `docs/decisions/plans/2026-09-13-napkin-m0-risk-validation.md`
**Spec 風險表:** `docs/decisions/specs/2026-09-13-napkin-design.md` §10
**實驗程式碼（已刪除，可從 git 歷史取回）:** `7e14330` `5a092f6` `7edc0cd`

## 1. fcitx5 中文輸入（input_probe）

| 檢查 | 結果 | 觀察 |
|---|---|---|
| A. 組字中的內容（preedit）顯示在文字框裡 | PASS | 組字「ni hao」顯示在文字框內 |
| B. 候選字視窗出現在文字框附近 | PASS | fcitx5 候選字視窗出現在文字框正下方 |
| C. 選字後文字框出現「你好」 | PASS | 按空白鍵後「你好」正確提交到文字框 |
| D. log 出現 `IME  Preedit` 與 `IME  Commit` | PASS | 日誌顯示 `IME  Preedit` 事件和 `IME  Commit("你好")` 事件 |

**結論：** 主方案：文字編輯使用 egui `TextEdit` 疊在元件上（spec §7.3）

## 2. 觸控板捏合縮放（input_probe、pinch_probe）

| 檢查 | 結果 | 觀察 |
|---|---|---|
| E. 捏合時 log 出現 `ZOOM` | FAIL | winit 0.30.13 只在 macOS／iOS 送出 `PinchGesture`，Wayland 後端沒有綁定 `zwp_pointer_gestures_v1`，所以 egui 收不到捏合事件；Hyprland 有提供 `zwp_pointer_gestures_v1`（version 3）。 |
| F. pinch_probe 捏合時 log 出現 `PINCH begin`／`update`／`end` | PASS | log 內有 3 行 `PINCH begin fingers=2`、339 行 `PINCH update ...`、3 行 `PINCH end cancelled=false`；cancelled 皆為 0、`EGUI  Zoom` 為 0 行，過程無 panic |
| G. 方塊隨捏合平順縮放，放開後停止 | PASS | 方塊隨手指開合平順縮放，放開後停止；截圖顯示累積 zoom 在同一次手勢中持續上升（例如 0.835 → 1.166），單次事件 factor 介於 1.000～1.034 之間 |
| H. 捏合期間一般滑鼠操作（移動、點擊）不受影響 | PASS | 捏合前後一般滑鼠移動與點擊皆正常運作，使用者回報「完全正常」 |

**結論：** spec 的主方案不成立：winit 0.30 在 Wayland 上不會送出捏合手勢（E）。改採 spec 未列出的新做法：napkin 在 eframe 的 Wayland 連線上自行綁定 `zwp_pointer_gestures_v1` 取得捏合縮放，同時保留 `Ctrl`+滾動。本結論取代 spec §7.1 縮放列與 §10 風險表第 4 列。正式程式碼必須在顯示環境不是 Wayland、或 compositor 未提供該協定時，退回只支援 `Ctrl`+滾動。

## 3. MSAA（canvas_probe）

| 檢查 | 結果 | 數值 |
|---|---|---|
| 程式沒有 panic | PASS | log 依序輸出 adapter 資訊、target format、多行 `callback rect px`、`screenshot saved to spikes/m0/out/canvas.ppm`，exit=0 |
| 斜線裁切區的顏色數 ≥ 3 | PASS | 5（`magick ... -format '%k'`），目視確認斜線邊緣平滑、有灰階漸層像素 |
| adapter backend | — | Vulkan（`Intel(R) Arc(tm) B390 (PTL)`，Mesa 26.2.2 開源驅動） |

**結論：** 主方案：整個視窗使用 MSAA 4×（`NativeOptions::multisampling = 4`）

## 4. glyphon 與圖形交錯繪製（canvas_probe）

| 檢查 | 結果 | 數值 |
|---|---|---|
| 紅框裁切區的顏色數 > 2（文字畫在紅框上） | PASS | 178，目視確認文字「Hello 手繪白板」清楚疊在紅框上 |
| 藍框裁切區的顏色數 = 1（文字被藍框蓋住） | PASS | 1，目視確認藍框內部是純色，文字右半部完全被蓋住 |

**結論：** 主方案（僅驗證單一文字群組）：glyphon 文字以「連續圖形一組、連續文字一組」的方式交錯繪製

## 5. 字型子集合併（build_fonts.py）

| 字型 | 子集數 | codepoint 數 | mismatches |
|---|---|---|---|
| napkin-hand | 7 | 561 | 0 |
| napkin-sans | 5 | 854 | 0 |
| napkin-code | 4 | 333 | 0 |

**結論：** 主方案：合併成單一 TTF（spec §6.3），但有已知落差，範圍已實測：napkin-sans 177,287 組同子集字母配對中有 3,013 組（約 1.7%）合併後的總前進寬度，跟瀏覽器分別載入各子集、依 unicode-range 切分文字 run 的模型不同；ASCII 與 Latin-1 不受影響（36,481 組中 0 組有落差）。所有落差都牽涉到同時出現在 Latin-Ext 子集與越南文子集的字母（例如 Ă、Ỳ–Ỹ、ơ、đ、Ư），例如 `ĂŴ` 在瀏覽器模型中是 677+1104，合併後變成 733+1104；合併後的字型也對一些瀏覽器從不 kern 的跨子集配對做了 kerning，例如 `YĂ` 瀏覽器模型是 601，合併後是 536。推測原因：U+0102 出現在 Nunito 全部五個子集的 cmap 裡，合併時每個 glyph 名稱只留一份，結果其中一個子集的 kerning 規則被套用到這個共用 glyph 上。napkin-hand 的每一組基本字母配對都吻合，只有分解重音符號序列不同；napkin-code 也只有分解重音符號序列不同（1 個單位的四捨五入誤差除外）。瀏覽器模型的限制：當兩個子集宣告同一個 codepoint 時，假設後宣告的子集勝出。`mismatches()` 這個檢查只比對 cmap、前進寬度與外框邊界，不涵蓋 GSUB／GPOS 排版表，spec §10「fonttools 無法乾淨合併」風險真正可能藏著 kerning／mark 定位問題的地方正是這裡。是否接受合併字型與瀏覽器行為的這個落差，或改用 spec §10 fallback 的逐子集獨立 family（會與瀏覽器行為完全一致），留給 M3 明確決定。

## 對後續里程碑的影響

- M4（文字編輯）：主方案，文字編輯採用 egui `TextEdit` 疊在元件上（spec §7.3）。
  - 現有證據只涵蓋平鋪視窗管理器下、固定位置的單一單行 TextEdit；M4 驗收需涵蓋多行 TextEdit、置於元件所在位置、字型與字級隨縮放倍率調整、以及疊加層在反覆隱藏／顯示切換 focus 之後仍正確運作。
- M3（縮放手勢）：採用 spec 未列出的新做法（非主方案，見上），自行綁定 `zwp_pointer_gestures_v1`，保留 `Ctrl`+滾動。
  - 做法：以 `wayland_backend::client::Backend::from_foreign_display` 在 eframe 的 `wl_display` 上建立 guest 連線，開自己的 event queue，在專用執行緒上 `blocking_dispatch`，收到捏合事件後呼叫 `request_repaint`；與 winit 同時讀取同一連線是安全的，靠的是 libwayland 的 `prepare_read`／`read_events` 鎖定。程式碼見 commit `7e14330` 的 `spikes/m0/src/bin/pinch_probe.rs`。
  - `scale` 是從手勢 `begin` 起算的累積值，M3 必須自行換算成每次事件的縮放倍率。
  - guest Backend 不能活得比 eframe 的 `wl_display` 久；在 spike 裡這只是因為 eframe 把 winit 的 `EventLoop` 放進 thread_local（`eframe-0.36.2/src/native/run.rs:62`，因為 `run_and_return` 預設為 `true`），而 winit 自己的連線在被 drop 時會呼叫 `wl_display_disconnect`（`wayland-backend-0.3.17/src/sys/client_impl/mod.rs:1149-1151`），加上目前 `blocking_dispatch` 執行緒沒有停止機制；M3 必須明確訂出、並自行負責這段生命週期。
  - 顯示環境不是 Wayland、或 compositor 未提供該協定時，須退回只支援 `Ctrl`+滾動。
- M3（反鋸齒）：主方案，整個視窗使用 MSAA 4×（`NativeOptions::multisampling = 4`）。
  - 所有自訂 pipeline 與 glyphon 的 `TextRenderer` 都必須使用相同的 sample count（4），否則 wgpu 驗證會失敗。
- M3（文字繪製）：主方案（僅驗證單一文字群組），glyphon 文字以「連續圖形一組、連續文字一組」的方式交錯繪製。
  - glyphon 畫完之後，畫布必須重新綁定自己的 pipeline 與 vertex buffer；目前只驗證過單一文字群組，多個群組需要多個共用同一 atlas 的 `TextRenderer`，M3 必須另外驗證。
- M3（字型載入）：主方案，合併成單一 TTF（spec §6.3），附帶上面記錄的已知 kerning 落差；合併字型與逐子集獨立 family 之間怎麼選，留給 M3 決定。
