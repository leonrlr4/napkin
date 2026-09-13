# napkin 設計規格

> Historical record, frozen 2026-09-13. Source code is authoritative; where this
> document and the code disagree, the code wins.

## 1. 這是什麼

napkin 是一個常駐的原生 Rust 手繪白板。在 Hyprland 上按 `SUPER+B`，它會以浮動
overlay 的形式出現：置中、佔螢幕 70%×70%、背後的桌面變暗；再按一次或按 `Esc`
收起。畫出來的內容就是 `.excalidraw` 檔，可以隨時複製成 PNG 貼到別處，也可以把
檔案丟進 excalidraw.com 繼續編輯。

### 1.1 v1 範圍

- **可建立與編輯的元件**：`rectangle`、`diamond`、`ellipse`、`arrow`（直線與曲線）、
  `line`、`text`、`freedraw`
- **手繪風格**：對同一個 seed，線條與 excalidraw.com 一致（判準見 §3、§9）
- **編輯**：點選、`Shift` 加選、框選、`Ctrl+A`、搬移、縮放、刪除、圖層上下移、
  複製／貼上／複製一份、undo／redo、橡皮擦
- **綁定**：箭頭綁定（箭頭端點跟著被綁元件移動）、容器文字（在形狀上雙擊輸入，
  自動置中與換行）
- **畫布**：無限平移與縮放
- **多畫布**：app 內 `Ctrl+P` 切換器，可開啟與建立
- **匯出**：PNG 到剪貼簿；`.excalidraw` 本身就是存檔格式
- **外觀**：深淺色與 UI 配色跟隨 omarchy 目前主題
- **中文輸入**：fcitx5

### 1.2 v1 明確不做

「讀到既有檔案時」一欄是硬性要求：不支援編輯，不代表可以弄壞資料。

| 不做的項目 | 讀到既有檔案時 |
|---|---|
| 旋轉控制點 | 旋轉過的元件正確顯示、可搬移、可刪除；不顯示縮放控制點 |
| elbow arrow（自動直角繞線） | 依儲存的點正確顯示、可整體搬移與刪除；端點不可編輯；綁定不跟隨 |
| 箭頭上的文字標籤 | 無法建立。既有標籤在箭頭搬移、或端點因綁定而改變時，依 Excalidraw 的規則重新定位（port commit `afa3a65` `getBoundTextElementPosition` 的箭頭分支） |
| `image`、`frame`、`magicframe`、`embeddable`、`iframe`、sticky note 及其他不認識的類型 | 資料原封保留；畫成虛線框加類型名稱；可選取、搬移、刪除 |
| 格線、吸附 | `appState` 中相關欄位原封保留 |
| 建立群組 | 既有群組在點選時整組選取 |
| 手寫風中文字型（Xiaolai） | 中文以系統 Noto Sans CJK 顯示；檔案內容不受影響 |
| 壓感筆 | freedraw 使用模擬壓力；既有檔案中的壓力值照用 |
| 切換器內改名、刪除 | 使用檔案管理員 |
| 舊版綁定格式（`focus`／`gap`） | 資料原封保留；拖動時不跟隨 |

## 2. 做決定時的環境

- Omarchy + Hyprland 0.56.2（lua 設定），`decoration:dim_special` 可用
- 螢幕 eDP-1 2880×1800 @ 120Hz，scale 1.25；GPU 為 Intel Arc B390（Vulkan）
- 輸入裝置：觸控板與 trackpoint；沒有繪圖板，也沒有觸控螢幕
- 輸入法：fcitx5
- Rust 1.98.1
- `SUPER+B` 當時沒有被佔用（`SUPER+D` 已被 dictionary plugin 佔用）
- omarchy 目前主題的色票：`~/.local/state/omarchy/current/theme/colors.toml`

## 3. 相容性基準

凡是「與 Excalidraw 一致」的判準，一律以下列版本為準：

| 對象 | 版本與來源 |
|---|---|
| Excalidraw | commit `afa3a65`（2026-09-10） |
| rough.js | `roughjs@4.6.4`，Excalidraw 在該 commit 釘選的版本（不是最新的 4.6.6） |
| perfect-freehand | `perfect-freehand@1.2.0` |
| fractional indexing | Excalidraw repo 內的 `packages/fractional-indexing`（3.3.0）。**npm 上的同名套件是不相干的 0.18.0 預發版，不能當基準** |
| 元件 id 格式 | `nanoid@3.3.3`（21 字元，字元集 `A-Za-z0-9_-`） |
| 字型 | 字型選單的三個選項：Excalifont（`fontFamily: 5`，SIL OFL 1.1；名稱表聲明「Excalifont is a trademark of Excalidraw」）、Nunito（`6`）、Comic Shanns（`8`，MIT）。來源是該 commit 的 `packages/excalidraw/fonts/` |

## 4. 架構

### 4.1 執行模型

- `napkin` 是常駐程式。登入時由 Hyprland autostart 啟動，靜默放進 `special:napkin`。
- `SUPER+B` 執行 `napkin-toggle`：如果沒有 `napkin` 行程在跑，先把它啟動到
  `special:napkin`，再執行 `togglespecialworkspace napkin`。
- Hyprland 視窗規則（比對 app_id `napkin`）：float、center、size 70% 70%。背景變暗
  用 `decoration:dim_special`。
- 關閉視窗（例如按 `SUPER+W`）等於存檔後正常結束，下次按熱鍵時由 `napkin-toggle`
  重新啟動。**不攔截關閉事件。**
- 收起時不重繪：egui 採事件驅動重繪，而看不見的 Wayland surface 不會收到 frame
  callback。
- Hyprland 設定寫在使用者自己的 `~/.config/hypr/`，repo 裡只提供範例片段。app 內
  **只有一處**直接呼叫 `hyprctl`：`Esc` 收起（§7.5）。

### 4.2 Crate 切分

Cargo workspace，依賴方向只有 `app → scene → rough`。

**`rough`**：逐行 port rough.js 4.6.4。
- 輸入：幾何形狀與 options（含 seed）。
- 輸出：ops（`move`／`lineTo`／`bcurveTo`），分成 `path`／`fillPath`／`fillSketch`
  三種 set。
- 零依賴，完全不知道 Excalidraw 的存在。
- 亂數產生器必須與 rough.js 完全相同：
  `seed = imul(48271, seed)`，輸出 `(seed & 0x7FFFFFFF) / 2^31`；seed 為 0 時的行為
  也要照原版。

**`scene`**：
- `.excalidraw` 的資料結構與 serde，含未知欄位保留（§5.2）。
- Excalidraw 的形狀規則：`generateRoughOptions`、`adjustRoughness`、圓角路徑、菱形
  頂點、箭頭頭部、elbow arrow 路徑、freedraw（perfect-freehand）。輸出每個元件的
  shape ops（純資料）。
- 編輯操作、綁定維護、fractional index、undo／redo。
- 互動狀態機：輸入抽象的指標／鍵盤事件，輸出場景修改。**不依賴 egui。**
- 深色模式色彩轉換（§6.6）。

**`app`**（binary 名稱 `napkin`）：
- eframe 視窗與 egui UI
- wgpu 畫布 renderer
- 把 egui 事件翻譯成 `scene` 的抽象事件
- 匯出、檔案 I/O 排程（debounce、原子寫入、外部修改偵測）
- 讀取主題

## 5. 資料模型與檔案

### 5.1 格式

原生存檔格式就是 `.excalidraw`：
`{ type: "excalidraw", version: 2, source, elements, appState, files }`。
記憶體中的結構與 JSON 一一對應，沒有中間轉換層。`files`（圖片二進位資料）原封保留。

### 5.2 不認識的東西一個欄位都不能丟

- 每個元件、頂層物件、`appState` 的 serde 結構都帶
  `#[serde(flatten)] extra: Map<String, Value>`，存檔時原樣寫回。
- 不認識的元件類型保存為原始 JSON，只額外解析顯示和搬移所需的欄位（`x`、`y`、
  `width`、`height`、`angle`、`index`、`isDeleted`、`groupIds`）。搬移時只改原始
  JSON 裡的 `x`／`y`。
- 刪除元件是設定 `isDeleted: true` 並遞增 `version`，跟 Excalidraw 的語意一致。讀進
  來的 `isDeleted` 元件不顯示，但存檔時保留。

**為什麼**：在 excalidraw.com 畫了含圖片的圖，丟進 napkin 改一個字再存檔，圖片卻
消失了。這種錯誤不會有任何錯誤訊息，只會悄悄吃掉資料。

### 5.3 新元件的欄位

- `id`：nanoid 格式
- `seed`：`[0, 2^31)` 的隨機整數，建立後永遠不變
- `index`：照 §3 的 fractional indexing 產生
- `version` 每次修改遞增；`versionNonce` 每次修改重新抽；`updated` 更新為當下的毫秒
  時間戳
- 其餘欄位的預設值照 commit `afa3a65` 的 `newElement` 與預設元件屬性

### 5.4 存放位置

| 用途 | 路徑 |
|---|---|
| 畫布檔案 | `~/Documents/napkin/<畫布名稱>.excalidraw` |
| 上次開啟的畫布 | `~/.local/state/napkin/last` |
| log | `~/.local/state/napkin/log`（每次啟動時覆蓋） |

- 叫出時開啟上次使用的畫布。第一次使用時自動建立 `scratch`。
- 視角存在 `appState.napkin = { scrollX, scrollY, zoom }`。如果檔案被
  excalidraw.com 重新存檔時丟掉了這個欄位，只會讓視角回到預設，不影響內容。

### 5.5 自動存檔

- 最後一次修改後停頓 500ms 寫檔；視窗失去焦點時立刻寫檔；正常結束前寫檔。
- 原子寫入：先寫到同目錄的暫存檔 → `fsync` → `rename`。

### 5.6 外部修改

視窗取得焦點時比對檔案的 mtime。如果比上次自己寫入的還新，就重新載入並清空 undo
歷史。因為失去焦點時一定已經存過檔，這時本地不會有未存的修改。

### 5.7 Undo／redo

- 每一步記錄「被改到的元件：改之前／改之後」。
- 最多 200 步，只放在記憶體中。
- 切換畫布、重新載入、重新啟動時清空。

### 5.8 綁定

**箭頭綁定**
- 格式：箭頭的 `startBinding`／`endBinding` 是 `{ elementId, fixedPoint, mode }`；被
  綁元件的 `boundElements` 含 `{ id, type: "arrow" }`。
- 建立：把箭頭端點拖到可綁定的元件上（v1 支援 `rectangle`、`diamond`、`ellipse`、
  `text`）。
- 跟隨：被綁元件移動或縮放時，重新計算箭頭對應的端點。`mode: "orbit"` 讓端點停在
  外框外、間距照 `getBindingGap`；`mode: "inside"` 讓端點位於 `fixedPoint`。演算法
  port 自 commit `afa3a65` 的 `binding.ts`，只取直線與曲線箭頭需要的部分。
- 刪除被綁元件時，清除箭頭上對應的 binding，保留箭頭本身。刪除箭頭時，把它從被綁
  元件的 `boundElements` 中移除。
- 兩端的資料必須保持一致：任何一端有記錄，另一端就必須有對應的記錄。

**容器文字**
- 格式：文字的 `containerId` 指向容器；容器的 `boundElements` 含
  `{ id, type: "text" }`。容器可以是 `rectangle`、`diamond`、`ellipse`。
- 行為：在容器上雙擊開始輸入。文字置中；容器移動時文字跟著移動；容器縮放時重新換行；
  文字高度超過容器時，容器自動長高。
- 換行規則 port 自 commit `afa3a65` 的 `textWrapping.ts`（包含 CJK 斷行），用與
  Excalidraw 相同的字型檔測量寬度。

## 6. 渲染管線

每一幀的時間預算：8.3ms（120Hz）。70% 的面板大約是 2016×1260 實體像素。

### 6.1 流程與快取

```
元件 ──① generate (scene)──▶ shape ops ──② tessellate (lyon)──▶ mesh ──③ draw──▶ 像素
          快取鍵 (id, version, 深淺色)          快取鍵 (id, version, 縮放級距)
```

- 快取鍵直接用元件的 `version`，不需要另外追蹤哪些元件被改過。
- 所有 mesh 放在共用的 vertex／index buffer 中，每個元件佔一段。
- ③ 每一幀只更新 camera 矩陣，照 `index` 順序畫出視窗內的元件。
- 平移時 ①② 完全不執行；拖動元件時只有該元件重跑 ①②。
- 縮放級距是 2 的次方。跨級距時，只對視窗內的元件重新 tessellate。
- 視窗外的元件用 bounding box 剔除。

### 6.2 線條

| rough.js set | 畫法 |
|---|---|
| `path` | lyon 描邊，圓角接頭、圓頭端點 |
| `fillSketch`（hachure、cross-hatch、zigzag） | lyon 描邊，線寬為 `fillWeight` |
| `fillPath` | lyon 填充。`curve`、`polygon`、`path` 用 evenodd，其他用 nonzero（照 rough.js canvas renderer） |

- 虛線與點線：照 Excalidraw 的 dash pattern（dashed `[8, 8 + strokeWidth]`、dotted
  `[1.5, 6 + strokeWidth]`）把路徑切段後再描邊。
- freedraw：perfect-freehand 產生輪廓多邊形，再填充。
- 反鋸齒：整個視窗 MSAA 4×。
- 透明度：`opacity` 為 100 的元件直接畫。小於 100 的元件先以不透明方式畫到離屏
  texture，再以該透明度合成。這跟 Excalidraw「每個元件先畫到自己的 canvas 再合成」
  一致，避免 rough 雙重描邊互相疊加變深。
- 不認識的元件類型：畫成虛線框，加上類型名稱。

### 6.3 文字

- 排版與字型 fallback 用 `cosmic-text`，GPU 繪製用 `glyphon`。
- 文字與圖形照圖層順序交錯繪製：把連續的圖形分成一組、連續的文字分成一組，依序畫出。
- 打包的字型：Excalidraw repo 中這三種字型都被切成多個 woff2 子集（Excalifont 7 個、
  Nunito 5 個、Comic Shanns 4 個），而且上游沒有公開的完整字型檔。所以由 repo 內的開發
  工具 script，從 §3 固定的 commit 取出子集，用 fonttools 解壓並合併成單一 TTF，commit
  進 repo 並附上原授權文字。
- **合併後的字型必須改名**：Excalifont 的名稱表聲明它是 Excalidraw 的商標，而 OFL 不授予
  商標使用權，所以修改（合併）後的版本不沿用原名，避免被當成官方字型。（Excalifont 並沒有
  宣告 OFL 的 Reserved Font Name。）三種字型統一改名為 `napkin-hand`（Excalifont）、`napkin-sans`（Nunito）、
  `napkin-code`（Comic Shanns）。`.excalidraw` 檔案記錄的是數字 `fontFamily`，所以改名
  不影響相容性。
- `fontFamily` 對應：`5`、`1`（Virgil）→ `napkin-hand`；`6`、`2`（Helvetica）、`7`、`9`、
  `10` → `napkin-sans`；`8`、`3`（Cascadia）→ `napkin-code`；其他未知值 → `napkin-hand`。
- 中文等打包字型沒有的字元，fallback 到系統的 Noto Sans CJK。
- 跨縮放級距時，用新的實際像素大小重新產生字形。

### 6.4 選取與提示

選取框、控制點、hover 高亮、綁定提示用 egui painter 畫在畫布上層，不進快取流程。

### 6.5 匯出

用同一條渲染管線畫到離屏 texture，讀回後編碼成 PNG（§7.4）。

### 6.6 深色模式

檔案永遠儲存淺色模式的顏色。深色模式在 ① 階段轉換：invert 93% 再 hue-rotate 180°，
port 自 commit `afa3a65` 的 `packages/common/src/colors.ts` `applyDarkModeFilter`。

### 6.7 效能目標（驗收標準）

| 情境 | 目標 |
|---|---|
| 按熱鍵到可以開始畫 | 下一幀（不含 Hyprland 動畫本身的時間） |
| 1000 個元件的場景，連續平移／縮放／拖動 10 秒 | 影格時間 p99 ≤ 8.3ms |
| 收起時 | 影格計數器不增加（沒有任何重繪） |
| 程式結束後重新啟動 | 從行程啟動到第一幀呈現 < 500ms |

## 7. 操作

### 7.1 快捷鍵

與 Excalidraw commit `afa3a65` 的 `Tools.tsx`、`actions/shortcuts.ts` 一致：

| 工具 | 鍵 | | 操作 | 鍵 |
|---|---|---|---|---|
| 選取 | `V` `1` | | 複製成 PNG | `Shift+Alt+C` |
| 矩形 | `R` `2` | | 複製／貼上／複製一份 | `Ctrl+C` `Ctrl+V` `Ctrl+D` |
| 菱形 | `D` `3` | | Undo／Redo | `Ctrl+Z` `Ctrl+Shift+Z` |
| 橢圓 | `O` `4` | | 上移／下移一層 | `Ctrl+]` `Ctrl+[` |
| 箭頭 | `A` `5` | | 全選 | `Ctrl+A` |
| 線 | `L` `6` | | 畫布切換器 | `Ctrl+P` |
| 手繪 | `P` `X` `7` | | 平移 | 雙指滑動、`Space`+拖曳、中鍵拖曳 |
| 文字 | `T` `8` | | 縮放 | `Ctrl`+滾動；winit 有傳 Wayland 捏合手勢時也支援捏合 |
| 橡皮擦 | `E` `0` | | 收起 | `Esc`（§7.5） |
| 手 | `H` | | | |

元件的複製貼上使用 Excalidraw 的剪貼簿格式（`type: "excalidraw/clipboard"`），所以
可以與 excalidraw.com 互相貼上。

### 7.2 版面

- 上方中間：工具列。
- 左側：屬性面板，有選取或正在使用工具時顯示。內容有線條色、填充色、填充樣式、線寬、
  線條樣式、潦草度、邊角、箭頭頭部、字型、字級、透明度。
- 右上角：畫布名稱，點擊後開啟切換器。
- 調色盤使用 Excalidraw 的預設色票。

### 7.3 文字編輯

編輯中的文字是疊在元件位置上的 egui `TextEdit`，字型與字級跟著縮放倍率調整。編輯
結束後才寫回元件。中文輸入走 egui → winit → Wayland text-input 協定。

### 7.4 畫布切換器與匯出

**切換器**（`Ctrl+P`）
- 列出 `~/Documents/napkin/` 裡的畫布，照 mtime 由新到舊排序，支援模糊搜尋。
- 輸入不存在的名稱後按 Enter，會建立新畫布。
- 無法解析的檔案標示為「無法開啟」並顯示原因。

**PNG 到剪貼簿**（`Shift+Alt+C`）
- 有選取時只匯出選取的元件，否則匯出全部元件。
- 2× 解析度，padding 10px（Excalidraw 的 `DEFAULT_EXPORT_PADDING`）。
- 包含背景，深淺色跟目前畫面一致。
- MIME 類型 `image/png`。napkin 常駐，所以剪貼簿資料在程式結束前都拿得到。

### 7.5 `Esc`

- 先依序取消：結束文字編輯 → 放棄正在畫的形狀 → 取消選取。
- 沒有東西可以取消時，執行 `hyprctl dispatch togglespecialworkspace napkin` 收起畫布。

### 7.6 主題

- 讀取 `~/.local/state/omarchy/current/theme/colors.toml`。
- `mode` 決定畫布是深色還是淺色；`background`、`foreground`、`accent`、`selection`、
  `muted` 等決定 egui 介面的配色。
- 啟動時與每次取得焦點時讀取。檔案不存在或無法解析時，使用內建的深色配色。

## 8. 錯誤處理

原則：寧可大聲失敗，也不要悄悄弄丟資料。

| 情況 | 處理 |
|---|---|
| 檔案無法解析 | 絕不覆寫原檔。在切換器中標示並顯示原因。如果是啟動時自動開啟的檔案，改開 `scratch` 並顯示通知 |
| 存檔失敗 | 內容保留在記憶體中。右上角持續顯示紅色警示，直到下一次存檔成功 |
| panic | 不嘗試存檔，因為狀態可能已經不一致，寫入反而會弄壞檔案。最多損失最後 500ms 的修改 |
| 剪貼簿寫入失敗、`hyprctl` 失敗 | 顯示 toast，程式繼續執行 |
| 主題檔讀取失敗 | 使用內建深色配色，寫入 log |

## 9. 測試與驗收

### 9.1 `rough`

- repo 內附一支 node script，用 `roughjs@4.6.4` 產生 JSON 基準檔（輸入 + options +
  seed → ops），產生後 commit 進 repo。
- 涵蓋每個 generator 函數 × 每種填充樣式 × 潦草度 0／1／2 × 多個 seed。
- 逐個數字比對，**絕對誤差 1e-9**。不要求每個 bit 相同，因為 V8 與 glibc 的
  `sin`／`cos` 在最後一位可能不同；亂數產生器是純整數運算，誤差不會累積放大。

### 9.2 `scene`

- **Round-trip**：一組真實的 `.excalidraw` 檔，經過 `讀入 → 寫出` 後，JSON 在語意上
  必須與原檔相等。比對時忽略 key 順序，並先把所有數字正規化成 f64 再比較（`1` 與
  `1.0` 視為相同）。
  - 這組檔案要在 excalidraw.com 上實際畫出來（Excalidraw repo 裡沒有現成的範例場景），
    至少要包含：圖片、frame、箭頭綁定、容器文字、箭頭文字標籤、elbow arrow、中文字、
    每種填充樣式、虛線與點線、旋轉過的元件、群組、已刪除的元件。
- **Excalidraw 形狀規則**：把 commit `afa3a65` 中 `generateRoughOptions`、
  `adjustRoughness`、箭頭頭部等函數原封複製到基準產生 script（MIT 授權，註明出處與
  commit），在 `roughjs@4.6.4` 上產生基準。
- **fractional index**：用 §3 指定的 repo 內原始碼產生基準。
- **freedraw**：用 `perfect-freehand@1.2.0` 產生基準。
- **Undo／redo**：property test，任意操作序列執行後全部 undo，場景必須回到初始狀態。
- **綁定**：搬移、縮放、刪除後，箭頭端點與容器文字的位置正確，且兩端資料一致。
- **互動狀態機**：用模擬事件驅動，驗證各工具的建立、選取與拖曳行為。

### 9.3 人工驗收清單（每個里程碑執行一次）

1. 按 `SUPER+B` 叫出與收起；背景變暗；`Esc` 可以收起；程式結束後按熱鍵會重新啟動。
2. 用 fcitx5 輸入中文：候選字窗位置正確，preedit 顯示正常。
3. 同一份參考檔在 napkin 與 excalidraw.com 並排比較，線條形狀一致。
4. 在 excalidraw.com 存一份含圖片的檔案 → 在 napkin 裡改一個字並存檔 → 丟回
   excalidraw.com，圖片仍在。
5. `Shift+Alt+C` 之後，在瀏覽器中可以貼上 PNG。
6. 1000 個元件的場景達到 §6.7 的影格時間目標，用 app 內建的隱藏影格時間面板量測。

## 10. 風險與提前驗證

以下項目排在實作計畫的最前面，每一項都有明確的「不通過時」處理方式。

| 風險 | 驗證方式 | 不通過時 |
|---|---|---|
| fcitx5 無法在 egui `TextEdit` 中正常輸入中文 | **第一個任務**：最小的 eframe 視窗 + `TextEdit`，在 Hyprland 上用 fcitx5 輸入 | 用 egui 的 `Event::Ime` 自己寫文字編輯元件。如果 winit 層就收不到 IME 事件，停下來重新評估整體設計 |
| eframe 的 wgpu 後端不支援 MSAA | 最小 paint callback 畫斜線，肉眼檢查 | 畫布先畫到自己的 4× MSAA 離屏 texture，resolve 後再合成 |
| glyphon 在 paint callback 中無法照圖層交錯繪製 | 與上一項同一個原型 | 用 cosmic-text 的 `SwashCache` 自建字形 atlas，把字形 quad 併入畫布的 vertex stream |
| winit 沒有傳遞 Wayland 捏合手勢 | 實測 | 只支援 `Ctrl`+滾動縮放 |
| fonttools 無法乾淨合併 woff2 子集（例如重複字形衝突） | 合併後逐一比對每個子集 cmap 中的字元都存在於合併結果 | 不合併，每個子集各自改名為獨立 family（如 `napkin-hand-0`…），由文字層依字元所在的 cmap 選擇 family |

## 11. 被否決的方案

- **omarchy-shell（quickshell）overlay plugin**：plugin 只能是 QML，無法載入 Rust
  渲染；QML 的 `Canvas`／`Shape` 撐不住大量手繪線條；而且會綁死 omarchy-shell 的版本。
  overlay 外觀改由 Hyprland 的 special workspace + `dim_special` 提供。
- **自己建立 wlr-layer-shell surface**：Hyprland 的視窗規則已經能做出相同外觀。
- **winit + wgpu + vello，UI 全部手刻**：工具列、文字輸入、清單都要自己寫，工作量是
  2 到 3 倍，而且這些都不在效能的關鍵路徑上。
- **iced**：`Canvas` 每次重繪都會重建 geometry，要繞過它做快取，比 egui 的 paint
  callback 彆扭。
- **`roughr` crate**：它用 `rand::StdRng`，不是 rough.js 的 LCG，所以同一個 seed 畫出
  的線條與 excalidraw.com 不同；而且 `StdRng` 不保證跨版本穩定，升級依賴會讓舊的圖
  變形。
- **`fractional_index` crate**：編碼方式與 Excalidraw 不同。
- **v1 打包 Xiaolai 字型**：Excalidraw repo 中的 Xiaolai 被切成 209 個網頁用的 woff2
  子集，不適合原生 app 直接使用。
- **合併後沿用 Excalifont 原名**：Excalifont 是 Excalidraw 的商標，OFL 不授予商標使用權。
- **攔截視窗關閉事件改成隱藏**：需要與 Hyprland 雙向耦合。改為關閉即結束，由
  `napkin-toggle` 負責重新啟動。
