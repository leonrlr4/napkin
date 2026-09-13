# napkin 里程碑路線圖

> Historical record, frozen 2026-09-13. Source code is authoritative; where this
> document and the code disagree, the code wins.

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`

每個里程碑有自己的實作計畫，等前一個里程碑完成才寫，因為後面的計畫要根據前面實際做出來的介面和驗證結果來寫。M1、M2 不受 M0 結果影響，M0 還在進行時就可以開始寫。

| 里程碑 | 產出 | 依賴 | 完成條件 |
|---|---|---|---|
| **M0 風險驗證** | spec §10 各項風險的結論；正式的字型建置工具與字型檔 | — | findings 紀錄已 commit，每項風險都標明走主方案還是替代方案 |
| **M1 `rough`** | rough.js 4.6.4 的 Rust port | — | node 產生的基準全部在 1e-9 誤差內通過（spec §9.1） |
| **M2 `scene` 檔案層與形狀規則** | `.excalidraw` 讀寫、保留未知欄位、新元件欄位、fractional index、Excalidraw 形狀規則、perfect-freehand、深色模式轉換 | M1 | round-trip 語料與形狀基準全部通過（spec §9.2） |
| **M3 `app` 唯讀檢視器** | eframe 視窗、wgpu 畫布管線與快取、文字與字型、半透明合成、主題；可以打開 `.excalidraw` 並平移、縮放 | M0、M2 | 人工驗收 #3；spec §6.7 平移／縮放的影格時間目標 |
| **M4 編輯** | 互動狀態機、所有工具、選取與變形、刪除、圖層順序、undo／redo、元件複製貼上、屬性面板、文字編輯、自動存檔、外部修改偵測 | M3 | spec §9.2 的 undo 與互動測試；人工驗收 #2、#4 |
| **M5 綁定** | 箭頭綁定、容器文字換行、箭頭標籤定位、elbow arrow 顯示 | M4 | spec §9.2 的綁定測試 |
| **M6 整合** | 畫布切換器、PNG 匯出、`Esc` 收起、`napkin-toggle`、Hyprland 設定、log 與錯誤提示 | M5 | spec §9.3 人工驗收清單全部通過 |

M1 是照著已知演算法逐行 port，難在「做」而不在「決定」。所以 M1 的計畫重點放在基準產生、測試框架、port 的順序和每一步的測試關卡，不會把整份 port 的程式碼重抄一遍在計畫裡。
