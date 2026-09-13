# napkin M0 風險驗證結果

> Historical record, frozen FROZEN_DATE. Source code is authoritative; where this
> document and the code disagree, the code wins.

**Plan:** `docs/decisions/plans/2026-09-13-napkin-m0-risk-validation.md`
**Spec 風險表:** `docs/decisions/specs/2026-09-13-napkin-design.md` §10
**實驗程式碼（已刪除，可從 git 歷史取回）:** SPIKE_COMMITS

## 1. fcitx5 中文輸入（input_probe）

| 檢查 | 結果 | 觀察 |
|---|---|---|
| A. 組字中的內容（preedit）顯示在文字框裡 | PASS | 組字「ni hao」顯示在文字框內 |
| B. 候選字視窗出現在文字框附近 | PASS | fcitx5 候選字視窗出現在文字框正下方 |
| C. 選字後文字框出現「你好」 | PASS | 按空白鍵後「你好」正確提交到文字框 |
| D. log 出現 `IME  Preedit` 與 `IME  Commit` | PASS | 日誌顯示 `IME  Preedit` 事件和 `IME  Commit("你好")` 事件 |

**結論：** 主方案：文字編輯使用 egui `TextEdit` 疊在元件上（spec §7.3）

## 2. 觸控板捏合縮放（input_probe）

| 檢查 | 結果 | 觀察 |
|---|---|---|
| E. 捏合時 log 出現 `ZOOM` | FAIL | winit 0.30.13 只在 macOS／iOS 送出 `PinchGesture`，Wayland 後端沒有綁定 `zwp_pointer_gestures_v1`，所以 egui 收不到捏合事件；Hyprland 有提供 `zwp_pointer_gestures_v1`（version 3）。 |

**結論：** 暫定：winit 層不支援。使用者表示捏合縮放很重要，追加 pinch_probe 實驗，由 napkin 自行在 eframe 的 Wayland 連線上綁定 `zwp_pointer_gestures_v1`；實驗失敗時，v1 只支援 `Ctrl`+滾動縮放。

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
