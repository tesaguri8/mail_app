# クレジット / 素材の入手先・ライセンス

**ステータス:** 実装中（背景写真は**仮素材**）
**目的:** アプリに同梱している外部素材の出所とライセンスを記録する。正式リリース前に、仮素材の差し替え要否をここで確認する。

---

## 背景写真（`src/renderer/assets/backgrounds/`）

背景写真システム（[UI_UX_DESIGN.md](UI_UX_DESIGN.md)）の**同梱サンプル**。読み込みは `src/renderer/config/backgrounds.ts`（Vite glob）で行う。

### 仮素材（Lorem Picsum 経由の Unsplash 写真）

- **サービス:** Lorem Picsum — https://picsum.photos （配信画像は **Unsplash** 由来）
- **ダウンロード URL 形式:** `https://picsum.photos/id/{id}/1600/1000.jpg`
- **各画像の元情報 API:** `https://picsum.photos/id/{id}/info`
- **ライセンス:** Unsplash License（https://unsplash.com/license ）＝ **無料・商用可・帰属不要**。ただし写真そのものを再配布して競合の写真サービスを作る用途は不可。アプリ同梱の背景としての利用は可。

| ファイル | Picsum ID | 作者 | Unsplash 元ページ |
|---|---|---|---|
| `01-river.jpg` | 1015 | Alexey Topolyanskiy | https://unsplash.com/photos/-oWyJoSqBRM |
| `02-canyon.jpg` | 1016 | Philippe Wuyts | https://unsplash.com/photos/_h7aBovKia4 |
| `03-peaks.jpg` | 1018 | Andrew Ridley | https://unsplash.com/photos/Kt5hRENuotI |
| `04-lake.jpg` | 1036 | Wolfgang Lutz | https://unsplash.com/photos/yOujaSETXlo |
| `05-waterfall.jpg` | 1039 | Andrew Coelho | https://unsplash.com/photos/VB-w_3dnyvI |
| `06-forest.jpg` | 1043 | Christian Joudrey | https://unsplash.com/photos/mWRR1xj95hg |

> 取得日: 2026-07-03。`{id}/1600/1000` で 1600×1000 にクロップ済み。帰属は不要だが、作者名を上表に残しておく。

### その他

| ファイル | 出所 | 備考 |
|---|---|---|
| `00-photo.jpg` | プロジェクト管理分（今回のダウンロード対象外） | 元は `src/renderer/assets/background.jpg`。出所未確認のため、正式化前に要確認。 |

---

## 差し替え方針（正式リリース前）

- 仮素材（`01`〜`06`）は正式採用の可否を確認し、必要なら自前・購入・別フリー素材へ差し替える。
- 差し替え・追加は `src/renderer/assets/backgrounds/` に画像を置くだけでよい（`config/backgrounds.ts` の glob が自動で拾う）。
- 素材を追加・変更したら、本ファイルの表も更新する。
