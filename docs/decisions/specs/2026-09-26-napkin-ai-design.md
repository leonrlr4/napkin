# napkin AI 介面設計規格

> Historical record, frozen 2026-09-26. Source code is authoritative; where this
> document and the code disagree, the code wins.

補充 `2026-09-13-napkin-design.md`（以下稱主 spec）。這份只寫 AI 介面，以及它對路線圖造成的調整；主 spec 其他部分不變。

## 1. 目標

讓 Claude Code 能讀、能畫、能看 napkin 正在開的畫布。你在終端機裡跟 Claude Code 說「把 ~/Projects/foo 的架構畫出來」或「照我畫的這張圖實作」，它透過 napkin 操作畫布，每一步都立刻出現在畫面上，你看得到圖一步步長出來，一批修改按一次 `Ctrl+Z` 就能復原。

主要對象是 Claude Code。其他會跑 shell 指令的 agent（codex、gemini 等）也能用同一組指令，但不為它們另外做事。

## 2. 使用方式

1. 在 napkin 按 `Ctrl+K`，旁邊開出一個浮動終端機，在 `~/Documents/napkin` 執行 `claude --continue --dangerously-skip-permissions --model sonnet`。這個資料夾裡沒有過對話時，`--continue` 沒有東西可以接，改成不帶它啟動（實際行為在實作時確認）。
2. 你在 Claude Code 裡打字。終端機原本就能用 fcitx5 輸入中文。
3. Claude 依照 napkin skill 的說明，用 `napkin` 子指令讀取與修改畫布。
4. 那個終端機還開著時再按 `Ctrl+K`，只會切到它；關掉之後再按，`--continue` 接回這個資料夾最近一次的對話。要開新對話就在 Claude Code 裡打 `/clear`。

`--dangerously-skip-permissions` 與 `--model sonnet` 是使用者指定的預設。前者讓 Claude 改檔案、執行指令都不先問，範圍不限於 `~/Documents/napkin`。固定用這個資料夾，是因為 Claude Code 的對話紀錄跟著啟動資料夾存，固定下來 `--continue` 才接得回去；要讓它看某個專案，在對話裡講路徑即可。

## 3. 架構

```
Claude Code ──shell──▶ napkin <子指令> ──Unix socket──▶ 正在執行的 napkin ──▶ Editor
```

### 3.1 控制 socket

- napkin 啟動時在 `$XDG_RUNTIME_DIR/napkin.sock` 開一個 Unix socket，權限 0600。`--bench` 模式不開。
- 協定：每個連線送一行 JSON 請求、收一行 JSON 回應，然後關閉。
- 請求由背景執行緒收下，經 channel 交給 UI 執行緒，並呼叫 `request_repaint` 喚醒它；UI 執行緒在下一幀處理請求、改 `Editor`，再把回應送回背景執行緒。`Editor` 仍然只在 UI 執行緒上動，不加鎖。
- socket 已經被另一個還活著的 napkin 佔用時，這個 napkin 不開 socket，照常運作並顯示通知。檔案存在但沒有人在聽（上次當掉留下的）時，刪掉重建。
- 畫布唯讀（主 spec §8 的無法解析檔案）時，讀取類請求照常回應，修改類請求回錯誤。

### 3.2 子指令

子指令和 GUI 是同一支 `napkin` 執行檔。napkin 沒有在執行時，子指令回錯誤「napkin is not running; open it with SUPER+N」，結束碼非 0，不會直接去改檔案。

| 指令 | 回應 |
|---|---|
| `napkin status` | 目前開的檔案絕對路徑、有沒有還沒存的修改、上次存檔失敗的訊息、元件數量、畫布是否唯讀 |
| `napkin scene` | 第一行是檔案路徑，之後每個未刪除元件一行精簡摘要：id、類型、`x y width height`、文字或標籤文字、`strokeColor`／`backgroundColor`、箭頭兩端接在哪個 id、群組。`--full` 改輸出完整元件 JSON |
| `napkin selection` | 選取的元件，格式同 `scene` |
| `napkin view` | 目前視窗看得到的場景範圍 `x y width height` 與 zoom |
| `napkin apply` | 從 stdin 讀一批操作（§4），回傳結果（§4.4） |
| `napkin render --out FILE.png [--selection \| --view]` | 把整張畫布、選取的元件、或目前視角畫成 PNG。背景色照畫布的 `viewBackgroundColor`，深色模式照目前主題 |

摘要格式要穩定、好讀、省 token；確切格式在實作計畫裡定，skill 照實作寫。

### 3.3 skill

- 放在 repo 的 `skills/napkin/SKILL.md`（英文，照 repo 的程式碼語言慣例），用 symlink 裝到 `~/.claude/skills/napkin`，在任何資料夾啟動的 Claude Code 都讀得到。
- 內容：
  - 開始前先跑 `napkin status` 和 `napkin view`，新元件放在看得到的範圍裡。
  - `apply` 的格式與例子。
  - 畫得好看的慣例：框的大小、間距、對齊、配色、箭頭怎麼接。
  - 分批送，一批是一個有意義的步驟（先框、再箭頭、再標籤），讓使用者看到圖逐步出現。
  - 畫完 `render` 一張 PNG，用 Read 看過再回報。
  - 要理解使用者手畫的草圖時，同樣用 `render --selection` 或 `render` 看圖。

### 3.4 `Ctrl+K`

- napkin 用 `xdg-terminal-exec --app-id=org.napkin.agent --dir=$HOME/Documents/napkin` 開終端機執行 claude。
- 已經有 app-id 為 `org.napkin.agent` 的視窗時，改成聚焦它（`hyprctl clients -j` 查、`hl.dsp.focus` 聚焦）。
- 終端機視窗的浮動位置與大小屬於使用者的 Hyprland 設定（`~/.config/hypr/windows.lua`），不在 repo 裡；實作時一併加上，讓它和 napkin 並排不重疊。

### 3.5 顯示目前的檔案

- 右上角現在只顯示檔名。改成完整路徑，家目錄縮寫成 `~`，資料夾部分用淡色、檔名用正常顏色；點一下把絕對路徑複製到剪貼簿。
- 視窗標題改成 `{完整路徑} - napkin`。

## 4. `apply` 的格式

### 4.1 一批

```json
{"ops": [ {...}, {...} ]}
```

整批驗證通過才套用，任何一筆有錯就整批不動，回應列出每個錯誤的位置（第幾筆、哪個欄位）和原因。套用成功的一批是一步 undo，跟使用者自己的一個手勢一樣。使用者在 Claude 畫圖時可以繼續操作；某一批要改的元件已經被使用者刪掉，就是那一批驗證失敗。

### 4.2 `add`：skeleton 格式

照 Excalidraw `packages/element/src/transform.ts` 的 `ExcalidrawElementSkeleton` 與 `convertToExcalidrawElements`（釘選 commit），napkin 支援其中這些：

- `rectangle`、`diamond`、`ellipse`：`x`、`y`、`width`、`height`，可選 `label: {text, fontSize?, fontFamily?, textAlign?, verticalAlign?}`。
- `text`：`x`、`y`、`text`，可選 `fontSize`、`fontFamily`、`textAlign`。寬高由 napkin 量測。
- `line`、`arrow`：`x`、`y`，`points` 或 `width`／`height`；arrow 可選 `start`／`end: {id}`，指向同一批或既有的元件。
- `freedraw`：`x`、`y`、`points`。
- 所有類型：可選 `id`、`strokeColor`、`backgroundColor`、`fillStyle`、`strokeWidth`、`strokeStyle`、`roughness`、`opacity`、`groupIds`。

其餘欄位照 M4a 新元件的預設值補上（`ItemStyle`、seed、`versionNonce`、fractional index 等），亂數走 `scene::env::Env`。

不支援的寫法直接回錯誤，不猜意思：箭頭或線的 `label`（主 spec §1.2 不建立箭頭標籤）、`start`／`end` 帶 `type` 要求順便建立新元件、`image`、`frame` 與其他主 spec 不能建立的類型。

`id` 是這一批裡的代號，napkin 一律產生新的 nanoid，回應裡給出代號與真正 id 的對應。同一批裡的 arrow 可以用代號指向同一批新增的形狀。

### 4.3 `update` 與 `delete`

- `update`：`{"op": "update", "id": ..., "set": {...}}`。可改：`x`、`y`、`width`、`height`、`points`、`text`（文字元件或容器的標籤）、樣式欄位。改大小或位置時，綁定的標籤照 M4a 的 `bound_text_position` 重新置中。`Element::Raw` 只能改主 spec §5.2 允許的欄位，其他欄位回錯誤。
- `delete`：`{"op": "delete", "ids": [...]}`，照 M4a `delete_selection` 的規則處理綁定與容器文字。

所有修改都呼叫 `bump_version`，只在值真的改變時呼叫。

### 4.4 回應

成功時回傳新元件的代號對應、被改與被刪的 id；失敗時回傳錯誤清單。回應也帶當時的 `revision`，skill 不需要用到，留給除錯。

### 4.5 文字

- 標籤與文字元件的寬高照 Excalidraw `measureText`：寬是各行寬度的最大值，高是行數 × `lineHeight` × `fontSize`。字寬用 napkin 內建的字型量；中文用系統 Noto Sans CJK 顯示（主 spec §1.2），所以含中文的元件在 excalidraw.com 打開時寬度會有差異，這是既有的已知差異。
- 不自動換行。要換行由 Claude 在文字裡放 `\n`。容器不會因文字變長而長高；skill 要求 Claude 自己把框開得夠大。自動換行、容器自動長高仍然在原本排定的里程碑。
- 標籤位置用 M4a 已經 port 的 `bound_text_position`（`computeBoundTextPosition`）。

### 4.6 箭頭綁定

- `start`／`end` 指定的元件必須是 `rectangle`、`diamond`、`ellipse`。
- 綁定欄位（`startBinding`／`endBinding` 的格式，含 `fixedPoint`）與形狀的 `boundElements` 照釘選 commit 的 `bindBindingElement` 系列建立，端點落在形狀外框上的位置照 Excalidraw 算。
- 被綁的形狀之後移動時箭頭跟著走，仍然是原本「綁定跟隨」里程碑的工作，這裡不做。M4a 已經處理的兩件事照舊：刪除時清理綁定、拖動箭頭離開時解除綁定。

## 5. 路線圖調整

AI 介面插在 M4a 之後，成為新的 M5。原 M4b 維持 M4b，但排到 AI 之後；原 M5（綁定跟隨、容器文字）改稱 M6，原 M6（overlay、畫布切換器、匯出）改稱 M7。

新 M5 的範圍：

1. socket 與子指令（§3.1、§3.2）。
2. skeleton 轉換：標籤、文字量測、箭頭綁定的建立（§4）。
3. PNG 輸出（`render`）。主 spec 的「複製成 PNG 到剪貼簿」之後共用這條路徑。
4. skill（§3.3）與 `Ctrl+K`（§3.4）。
5. 顯示目前的檔案（§3.5）。

先做 AI 的理由：它需要的文字量測與綁定建立，M4b 的文字工具和原 M5 本來就要做，先做不會浪費；AI 可用之後，後續每個功能 Claude 都能馬上用到。代價是工具列與文字工具再晚一個里程碑。

## 6. 測試與驗收

- 請求處理寫成不依賴 socket 與 GUI 的函數（輸入 `Editor` 與請求、輸出回應），用單元測試鎖住：整批驗證失敗時場景不變、成功時一批一步 undo、代號對應、`Raw` 元件的限制、唯讀畫布拒絕修改。
- socket 與子指令用整合測試：起一個只有 socket 與 `Editor`、沒有視窗的 server，實際執行 `napkin` 子指令。
- skeleton 轉換裡確定性的部分（箭頭端點、綁定欄位、標籤位置），能用現有 `tools/baseline/scene` 的方式從釘選的 Excalidraw 原始碼產生 JS 基準就產生；文字量測用 Rust 單元測試。
- `render` 用 GPU 測試（沿用 `crates/app/tests/support` 的離螢幕渲染與全域鎖）。
- 人工驗收：
  1. 在 napkin 按 `Ctrl+K`，請 Claude 畫一張五個方塊、四條箭頭的流程圖，圖在 napkin 裡分批出現，完成後按 `Ctrl+Z` 能一批一批復原。
  2. 自己手畫一張草圖、選取它，請 Claude 說明它看到什麼，內容正確。
  3. Claude 畫的檔案拖進 excalidraw.com，元件、標籤、箭頭綁定都正確，拖動形狀時箭頭跟著走。
  4. 右上角與視窗標題顯示完整路徑，點路徑能複製。

## 7. 被否決的方案

- **MCP server**：工具定義常駐在每個 session 的 context 裡，而「怎麼畫得好」這類大段說明沒有地方放。socket 與子指令做好之後，要包一層 MCP 很便宜，真的需要時再加。
- **napkin 內建輸入框、背景執行 Claude、按鈕接回 session**：要做 egui 內的中文輸入、背景程序管理、狀態顯示、session id 存檔、yazi 選資料夾，換來的只是不用多開一個終端機。直接開終端機跑互動式 Claude Code 就有這些功能。
- **在最後聚焦的終端機資料夾啟動 Claude**：napkin 隨時可以用 SUPER+N 叫出，那時不一定有相關的終端機，資料夾不可靠。
- **只改檔案、不連動正在開的 napkin**：重新載入會清掉 undo，napkin 有未存修改時也不會載入，Claude 也看不到選取。
