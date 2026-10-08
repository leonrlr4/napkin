# napkin 旋轉與圖片設計規格

> Historical record, frozen 2026-10-09. Source code is authoritative; where this
> document and the code disagree, the code wins.

這份規格修改主 spec（`2026-09-13-napkin-design.md`）§1.2 的兩列，新增兩個里程碑：M6c 旋轉、M6d 圖片。兩者都排在 M6b（箭頭綁定）之前，使用者 2026-10-09 決定。行為的基準仍是 Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`。

## 1. 為什麼

使用者驗收 M6a 時指出三個缺少的關鍵功能：刻意旋轉元件、貼上圖片、在文字編輯框裡用滑鼠或 Shift 選取文字後複製。前兩個是主 spec §1.2 明確列為 v1 不做的項目，這份規格把它們納入。第三個是缺陷（egui `TextEdit` 本身支援選取），另外修，不在這份規格裡。

## 2. 對主 spec §1.2 的修改

| 原本 | 改成 |
|---|---|
| 旋轉控制點：不做；旋轉過的元件不顯示縮放控制點 | 有旋轉控制點；旋轉過的元件照常縮放（M6c） |
| `image`：資料原封保留，畫成虛線框 | 可貼上、拖放、顯示、搬移、縮放、旋轉、刪除（M6d）；`frame` 等其他類型維持原狀 |

M4a 計畫的決定 10（「有旋轉過元件的多選不顯示控制點」）一併取消。

## 3. M6c：旋轉

**控制點。** 照 `transformHandles.ts`：選取框上緣上方一個圓形旋轉控制點，單選與多選都有。拖動時繞選取框中心旋轉，Shift 鎖定在 15° 的倍數（`SHIFT_LOCKING_ANGLE`）。單選照 `rotateSingleElement`，多選照 `rotateMultipleElements`。游標在控制點上顯示旋轉游標。整個拖動是一步 undo。

**哪些元件可以旋轉。** rectangle、diamond、ellipse、text、freedraw、image、多點的 line／arrow。兩點的 line／arrow 不顯示旋轉控制點（Excalidraw 選取兩點線段時不顯示選取框，只顯示端點）。容器的標籤跟著容器轉（標籤 `angle` 等於容器的 `angle`，位置由 `bound_text_position` 算）。箭頭標籤 `angle` 維持 0。`Element::Raw` 不能旋轉，選取裡有 Raw 時不顯示旋轉控制點；elbow arrow 也不能旋轉。

**旋轉後的縮放。** M4a 只 port 了 `angle == 0` 會走到的縮放分支。M6c 把 `resizeSingleElement`、`resizeSingleTextElement`、`resizeMultipleElements`、`getResizedOrigin`、`getNextSingleWidthAndHeightFromPointer` 等函數裡與角度有關的部分補齊，旋轉過的元件顯示縮放控制點並沿自己的軸縮放；多選裡有旋轉過的元件時照 Excalidraw 強制等比例。

**點選與外框。** 碰撞判定與外框計算 M4a 已經處理旋轉（`collision.rs`、`geometry.rs`），不需要改。控制點的位置與命中判定要照 `getTransformHandlesFromCoords` 的旋轉版本。

**正確性。** 新增 scene 基準群組，由打包的 Excalidraw 原始碼產生：`rotateSingleElement`／`rotateMultipleElements` 的結果，以及旋轉過的元件經過各個控制點縮放後的 `x`、`y`、`width`、`height`、`angle`、`points`。比對方式與既有群組相同（絕對誤差 1e-9）。

**不做。** 屬性面板的旋轉欄位、鍵盤旋轉、Alt 拖動的特殊行為（Excalidraw 沒有），以及 crop 工具。

## 4. M6d：圖片

**來源。**
- `Ctrl+V`：用 `wl-clipboard-rs`（Wayland data-control 協定，在行程內讀）列出剪貼簿的 MIME 類型。有圖片類型就照 `image/png`、`image/jpeg`、`image/webp`、`image/gif` 的順序取第一個，讀出原始位元組；沒有圖片才走現有的文字貼上（純文字或 Excalidraw 剪貼簿 JSON）。讀不到剪貼簿（不是 Wayland、compositor 不支援 data-control）時退回只貼文字，並在 log 記一行。
- 拖放：egui 的 `dropped_files`，接受上述格式的檔案，多個檔案照 `positionElementsOnGrid` 排列。非圖片檔忽略。
- 位置：照 `insertImages`，以游標（貼上）或放下的位置（拖放）為中心。

**儲存（與 excalidraw.com 互通）。**
- 元件是型別化的 `image`：`fileId`、`status`（建立後為 `"saved"`）、`scale`（`[1, 1]`）、`crop`（`null`），其他欄位照 `newImageElement`。
- 檔案資料放在 `.excalidraw` 的 `files`：`{id, mimeType, dataURL, created, lastRetrieved}`，`dataURL` 是 base64 data URL。
- `fileId` 是原始檔案位元組的 SHA-1 十六進位字串（`generateIdFromFile`），所以同一張圖貼兩次只存一份。
- 照 Excalidraw 的預設 `imageOptions`：長邊超過 1440 px 先等比例縮小再存（`resizeImageFile`），縮小後仍超過 4 MiB 就拒絕，顯示「圖片太大」的通知。縮小時重新編碼成原本的格式；格式無法編碼時用 PNG。
- 複製／貼上圖片元件時，Excalidraw 剪貼簿 JSON 帶上對應的 `files`（`serializeAsClipboardJSON`），napkin 內部與 excalidraw.com 之間都能互貼。
- 刪除圖片元件不刪 `files` 裡的資料（Excalidraw 也不立即刪）。

**大小。** 先放一個佔位元件，解碼完成後照 `getImageNaturalDimensions` 設成自然尺寸，高度上限是畫布高度的一半（換算成場景單位），寬度照比例。整個插入是一步 undo。

**縮放與旋轉。** 圖片縮放是否預設等比例、Shift 是否反轉，照 Excalidraw 的 `transformElements` 與 `resizeElements.ts` 對 image 的處理，以 JS 為準。旋轉照 M6c。

**渲染。**
- 用 `image` crate 解碼 png、jpeg、webp、gif（只取第一格），上傳成 wgpu texture，以 `fileId` 快取；解碼在背景執行緒做，不阻塞 UI。
- 依圖層順序與圖形、文字交錯繪製（主 spec §6.3 的分組再加上圖片）。套用 `opacity`、`angle`、`crop`。深色模式不反轉圖片顏色（Excalidraw 的 `shouldInvertImage` 只對 SVG 成立；napkin 不支援 SVG，所以一律不反轉）。
- `files` 裡沒有資料、或解碼失敗的圖片，畫成現有的虛線佔位框。
- AI 介面的 `napkin render` 走同一套渲染，所以截圖裡看得到圖片。

**不做。** SVG（需要另外的向量光柵化器；貼上或拖放 SVG 時顯示「不支援 SVG」）、crop 工具、把圖片元件複製成 PNG 給其他程式（M7 的 PNG 匯出處理）、AI batch 介面新增圖片、動畫 GIF。

**新依賴。** `wl-clipboard-rs`、`image`（只開 png、jpeg、webp、gif）、一個 SHA-1 實作（`sha1` crate）。都只給 `app`，`scene` 只處理 `files` 的資料結構與元件欄位，不解碼圖片。

## 5. 測試

- M6c：上述 scene 基準群組；編輯器測試涵蓋拖動旋轉控制點、Shift 鎖角、旋轉後縮放、undo 一步、兩點箭頭沒有旋轉控制點。
- M6d：`fileId` 的 SHA-1 對照已知值；`files` 讀寫 round-trip 與 corpus 測試（語料已有含圖片的檔案）中圖片元件不再退回 `Raw`；剪貼簿 JSON 帶 `files`；插入尺寸與 1440 px 縮小規則的單元測試；GPU 測試畫一張已知圖片並比對像素。
- 人工驗收：截圖後 `Ctrl+V` 貼進 napkin、從檔案管理員拖一張照片進來、旋轉並縮放、存檔後丟進 excalidraw.com 圖片仍在且位置相同。
